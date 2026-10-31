package ai.storyteller.photocraft

import com.google.androidgamesdk.GameActivity

/**
 * Thin Android host. Rendering, editing state, tools and commands stay in the Rust application.
 *
 * Android-specific capabilities such as the Storage Access Framework will be added here through
 * narrow platform bridges rather than reimplementing editor behavior in Kotlin.
 */
class PhotoCraftActivity : GameActivity() {
    companion object {
        init {
            System.loadLibrary("main")
        }
    }
}
