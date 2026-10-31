package ai.storyteller.photocraft

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import com.google.androidgamesdk.GameActivity
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileInputStream
import java.io.IOException
import java.util.UUID
import java.util.concurrent.Executors

/**
 * Thin Android host. Editing and rendering stay in Rust; this class owns Android document-picker
 * and ContentResolver calls only.
 */
class PhotoCraftActivity : GameActivity() {
    companion object {
        private const val SAF_REQUEST_CODE = 0x5A31
        private const val BUFFER_BYTES = 64 * 1024
        private const val LIFECYCLE_PAUSED = 1
        private const val LIFECYCLE_RESUMED = 2
        private const val LIFECYCLE_MEMORY_PRESSURE = 3

        init {
            System.loadLibrary("main")
        }
    }

    private val safIo = Executors.newSingleThreadExecutor()
    private var activeSafRequestId: Long? = null

    private external fun nativeOnSafResult(requestId: Long, payload: String)
    private external fun nativeOnLifecycleEvent(event: Int)

    /**
     * Called by Rust on the Android UI thread. The callback is always delivered asynchronously
     * after the user has selected or cancelled a document.
     */
    fun launchPhotoCraftSafPicker(requestId: Long, requestJson: String) {
        runOnUiThread {
            if (activeSafRequestId != null) {
                nativeOnSafResult(requestId, errorPayload("Another Android document picker is already open"))
                return@runOnUiThread
            }

            val request = try {
                JSONObject(requestJson)
            } catch (error: Exception) {
                nativeOnSafResult(requestId, errorPayload("Invalid document-picker request: ${error.message}"))
                return@runOnUiThread
            }

            val kind = request.optString("kind")
            val intent = when (kind) {
                "open" -> buildOpenIntent(request)
                "save" -> buildSaveIntent(request)
                else -> {
                    nativeOnSafResult(requestId, errorPayload("Unsupported Android document-picker request"))
                    return@runOnUiThread
                }
            }

            activeSafRequestId = requestId
            saveRequests[requestId] = kind == "save"
            try {
                val title = request.optString("title")
                startActivityForResult(
                    if (title.isNotBlank()) Intent.createChooser(intent, title) else intent,
                    SAF_REQUEST_CODE
                )
            } catch (error: Exception) {
                activeSafRequestId = null
                saveRequests.remove(requestId)
                nativeOnSafResult(requestId, errorPayload("Could not open Android document picker: ${error.message}"))
            }
        }
    }

    private fun buildOpenIntent(request: JSONObject): Intent {
        val extensions = request.optJSONArray("extensions").toStringList()
        val mimeTypes = extensions.mapNotNull(::mimeForExtension).distinct()
        val primaryMime = when {
            mimeTypes.isEmpty() -> "*/*"
            mimeTypes.size == 1 -> mimeTypes.first()
            else -> "*/*"
        }
        return Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = primaryMime
            if (mimeTypes.size > 1) putExtra(Intent.EXTRA_MIME_TYPES, mimeTypes.toTypedArray())
            putExtra(Intent.EXTRA_ALLOW_MULTIPLE, request.optBoolean("multiple", false))
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
    }

    private fun buildSaveIntent(request: JSONObject): Intent {
        val mime = request.optString("mimeType").ifBlank { "application/octet-stream" }
        val name = safeFileName(request.optString("fileName").ifBlank { "Untitled.psd" })
        return Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = mime
            putExtra(Intent.EXTRA_TITLE, name)
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
    }

    @Deprecated("Android document-picker callback")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != SAF_REQUEST_CODE) return

        val requestId = activeSafRequestId
        activeSafRequestId = null
        if (requestId == null) return
        val isSave = saveRequests.remove(requestId) == true

        if (resultCode != Activity.RESULT_OK || data == null) {
            nativeOnSafResult(requestId, JSONObject().put("kind", "cancel").toString())
            return
        }

        val selectedUris = selectedUris(data)
        if (selectedUris.isEmpty()) {
            nativeOnSafResult(requestId, errorPayload("Android returned no selected document"))
            return
        }

        val flags = data.flags and (
            Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        )
        selectedUris.forEach { uri ->
            if (flags != 0) {
                try {
                    contentResolver.takePersistableUriPermission(uri, flags)
                } catch (_: SecurityException) {
                    // Some document providers grant transient access only. The current operation
                    // can still continue, but subsequent saves may be rejected by that provider.
                }
            }
        }

        safIo.execute {
            try {
                val result = if (isSave) {
                    makeSaveTarget(selectedUris.single())
                } else {
                    makeOpenResult(selectedUris)
                }
                nativeOnSafResult(requestId, result.toString())
            } catch (error: Exception) {
                nativeOnSafResult(requestId, errorPayload("Could not access selected document: ${error.message}"))
            }
        }
    }

    // The pending mode is tracked separately so the result handler cannot mistake a one-file Open
    // operation for Save.
    private val saveRequests = java.util.concurrent.ConcurrentHashMap<Long, Boolean>()

    private fun isSaveResult(data: Intent, requestId: Long): Boolean {
        return saveRequests.remove(requestId) == true
    }

    private fun selectedUris(data: Intent): List<Uri> {
        val clip = data.clipData
        if (clip != null && clip.itemCount > 0) {
            return (0 until clip.itemCount).mapNotNull { index -> clip.getItemAt(index).uri }
        }
        return listOfNotNull(data.data)
    }

    private fun makeOpenResult(uris: List<Uri>): JSONObject {
        val imports = File(filesDir, "PhotoCraft/SAF/Imports")
        if (!imports.exists() && !imports.mkdirs()) {
            throw IOException("could not create private import directory")
        }

        val files = JSONArray()
        val created = mutableListOf<File>()
        val createdDirectories = mutableListOf<File>()
        try {
            uris.forEach { uri ->
                val name = safeFileName(displayName(uri) ?: uri.lastPathSegment ?: "Imported file")
                // One document per directory lets Rust safely rekey a grant if PhotoCraft
                // normalizes that document's extension (e.g. ".PSD" to ".psd").
                val importDirectory = File(imports, UUID.randomUUID().toString())
                if (!importDirectory.mkdirs()) {
                    throw IOException("could not create a private import working directory")
                }
                createdDirectories += importDirectory
                val destination = File(importDirectory, name)
                // Track the destination before opening the provider stream so a partial file is
                // also removed if the provider fails midway through the copy.
                created += destination
                val input = contentResolver.openInputStream(uri) ?: throw IOException("provider returned no input stream")
                input.use { source ->
                    destination.outputStream().buffered(BUFFER_BYTES).use { target ->
                        source.copyTo(target, BUFFER_BYTES)
                        target.flush()
                    }
                }
                files.put(
                    JSONObject()
                        .put("path", destination.absolutePath)
                        .put("uri", uri.toString())
                        .put("name", name)
                )
            }
        } catch (error: Exception) {
            created.forEach { it.delete() }
            createdDirectories.forEach { it.delete() }
            throw error
        }
        return JSONObject().put("kind", "open").put("files", files)
    }

    private fun makeSaveTarget(uri: Uri): JSONObject {
        val exports = File(filesDir, "PhotoCraft/SAF/Exports")
        if (!exports.exists() && !exports.mkdirs()) {
            throw IOException("could not create private export directory")
        }
        val name = safeFileName(displayName(uri) ?: uri.lastPathSegment ?: "PhotoCraft document")
        val uniqueDirectory = File(exports, UUID.randomUUID().toString())
        if (!uniqueDirectory.mkdirs()) {
            throw IOException("could not create a unique SAF save directory")
        }
        val stagingFile = File(uniqueDirectory, name)
        return JSONObject()
            .put("kind", "save")
            .put("path", stagingFile.absolutePath)
            .put("uri", uri.toString())
            .put("name", name)
    }

    /**
     * Called by Rust after it has atomically written the export to app-private storage.
     * The provider write runs off the Android UI thread; Rust receives success only after flush
     * and close complete.
     */
    fun writePhotoCraftSafFile(requestId: Long, requestJson: String) {
        val request = try {
            JSONObject(requestJson)
        } catch (error: Exception) {
            nativeOnSafResult(requestId, writeError("Invalid provider-write request: ${error.message}"))
            return
        }

        val uriString = request.optString("uri")
        val sourcePath = request.optString("path")
        if (uriString.isBlank() || sourcePath.isBlank()) {
            nativeOnSafResult(requestId, writeError("Missing content URI or working-file path"))
            return
        }

        safIo.execute {
            try {
                val uri = Uri.parse(uriString)
                val output = contentResolver.openOutputStream(uri, "wt")
                    ?: throw IOException("document provider returned no output stream")
                FileInputStream(sourcePath).use { source ->
                    output.use { target ->
                        source.copyTo(target, BUFFER_BYTES)
                        target.flush()
                    }
                }
                nativeOnSafResult(requestId, JSONObject().put("kind", "write").put("ok", true).toString())
            } catch (error: Exception) {
                nativeOnSafResult(requestId, writeError("Provider write failed: ${error.message}"))
            }
        }
    }

    private fun displayName(uri: Uri): String? {
        return contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index >= 0 && !cursor.isNull(index)) cursor.getString(index) else null
            } else null
        }
    }

    private fun safeFileName(value: String): String {
        val name = value.substringAfterLast('/').substringAfterLast('\\')
            .map { char -> if (char.isISOControl()) '_' else char }
            .joinToString("")
            .replace(":", "_")
            .trim()
            .take(180)
        return name.ifBlank { "Imported file" }
    }

    private fun mimeForExtension(extension: String): String? {
        val normalized = extension.trim().trimStart('.').lowercase()
        val known = when (normalized) {
            "psd", "psb", "psdt" -> "image/vnd.adobe.photoshop"
            "pcraft" -> "application/vnd.photocraft.document"
            "svg", "svgz" -> "image/svg+xml"
            "heic", "heif" -> "image/heif"
            else -> MimeTypeMap.getSingleton().getMimeTypeFromExtension(normalized)
        }
        return known
    }

    private fun JSONArray?.toStringList(): List<String> {
        if (this == null) return emptyList()
        return (0 until length()).mapNotNull { index ->
            optString(index).takeIf { it.isNotBlank() }
        }
    }

    private fun errorPayload(message: String): String =
        JSONObject().put("kind", "error").put("message", message).toString()

    private fun writeError(message: String): String =
        JSONObject().put("kind", "write").put("ok", false).put("message", message).toString()

    /**
     * Request a recovery snapshot before Android backgrounds this Activity. The callback only
     * queues a Rust event and wakes the native loop; encoding and storage stay off this UI thread.
     */
    override fun onPause() {
        nativeOnLifecycleEvent(LIFECYCLE_PAUSED)
        super.onPause()
    }

    override fun onResume() {
        super.onResume()
        nativeOnLifecycleEvent(LIFECYCLE_RESUMED)
    }

    override fun onTrimMemory(level: Int) {
        if (level >= android.content.ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW) {
            nativeOnLifecycleEvent(LIFECYCLE_MEMORY_PRESSURE)
        }
        super.onTrimMemory(level)
    }

    override fun onDestroy() {
        val requestId = activeSafRequestId
        activeSafRequestId = null
        if (requestId != null) {
            saveRequests.remove(requestId)
            nativeOnSafResult(requestId, JSONObject().put("kind", "cancel").toString())
        }
        safIo.shutdown()
        super.onDestroy()
    }
}
