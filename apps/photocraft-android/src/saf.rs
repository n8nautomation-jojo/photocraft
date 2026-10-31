//! Storage Access Framework bridge for Android.
#![allow(unsafe_code)]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, atomic::{AtomicU64, Ordering}, mpsc::{self, Sender, RecvTimeoutError}},
    time::{Duration, Instant},
};
use jni::{
    JavaVM,
    objects::{Global, JObject, JString, JValue},
    refs::Cast,
    sys::{jlong, jobject, jstring},
};
use photocraft_ui_egui::{
    FileDialogAnswer, FileDialogFn, FileDialogReply, FileDialogRequest,
    jobs_ui::SaveCtl,
};

const PICKER_METHOD: &str = "launchPhotoCraftSafPicker";
const WRITE_METHOD: &str = "writePhotoCraftSafFile";
const METHOD_SIGNATURE: &str = "(JLjava/lang/String;)V";
const WRITE_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub(super) struct SafBridge { inner: Arc<SafInner> }

struct SafInner {
    app: winit::platform::android::activity::AndroidApp,
    grant_file: PathBuf,
    grants: Mutex<HashMap<String, String>>,
}

enum PendingRequest {
    Dialog { reply: FileDialogReply, inner: Arc<SafInner> },
    Write(Sender<Result<(), String>>),
}

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);
static REQUESTS: OnceLock<Mutex<HashMap<u64, PendingRequest>>> = OnceLock::new();

fn requests() -> &'static Mutex<HashMap<u64, PendingRequest>> {
    REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl SafBridge {
    pub(super) fn new(
        app: winit::platform::android::activity::AndroidApp,
        app_data_dir: &Path,
    ) -> Result<Self, String> {
        let saf_dir = app_data_dir.join("SAF");
        std::fs::create_dir_all(saf_dir.join("Imports")).map_err(|e| format!("create SAF imports directory: {e}"))?;
        std::fs::create_dir_all(saf_dir.join("Exports")).map_err(|e| format!("create SAF exports directory: {e}"))?;
        let grant_file = saf_dir.join("uri-grants.json");
        let grants = if grant_file.exists() {
            let bytes = std::fs::read(&grant_file).map_err(|e| format!("read SAF URI grants: {e}"))?;
            serde_json::from_slice(&bytes).map_err(|e| format!("parse SAF URI grants: {e}"))?
        } else { HashMap::new() };
        Ok(Self { inner: Arc::new(SafInner { app, grant_file, grants: Mutex::new(grants) }) })
    }

    pub(super) fn file_dialog(&self) -> FileDialogFn {
        let bridge = self.clone();
        Box::new(move |request, _frame, reply| bridge.show_picker(request, reply))
    }

    fn show_picker(&self, request: FileDialogRequest, reply: FileDialogReply) {
        let payload = match request {
            FileDialogRequest::Open { multiple, extensions, .. } => serde_json::json!({
                "kind": "open", "multiple": multiple,
                "extensions": extensions.unwrap_or_default(),
                "title": if multiple { "Open files" } else { "Choose a file" },
            }),
            FileDialogRequest::Save { suggested } => {
                let path = PathBuf::from(&suggested);
                let file_name = path.file_name().and_then(|v| v.to_str()).unwrap_or("Untitled.psd");
                let mime_type = path.extension().and_then(|v| v.to_str()).map(mime_for_extension).unwrap_or("application/octet-stream");
                serde_json::json!({ "kind": "save", "fileName": file_name, "mimeType": mime_type, "title": "Save PhotoCraft document" })
            }
            FileDialogRequest::Folder { .. } => {
                reply.send(Some(FileDialogAnswer::Unavailable("Android folder selection is not available yet.".into())));
                return;
            }
        };
        let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        lock(requests()).insert(request_id, PendingRequest::Dialog { reply, inner: self.inner.clone() });
        self.post_to_activity(request_id, PICKER_METHOD, payload.to_string());
    }

    fn post_to_activity(&self, request_id: u64, method: &'static str, payload: String) {
        let app = self.inner.app.clone();
        app.run_on_java_main_thread(Box::new(move || {
            let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
            let call = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
                let activity_ptr = app.activity_as_ptr() as jobject;
                // SAFETY: android-activity supplies an unowned global reference valid while app lives.
                let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&activity_ptr)? };
                let payload = env.new_string(payload)?;
                let args = [JValue::Long(request_id as jlong), JValue::Object(payload.as_ref())];
                env.call_method(activity.as_ref(), method, METHOD_SIGNATURE, &args)?;
                Ok(())
            });
            if let Err(error) = call {
                complete_error(request_id, format!("Android document picker could not start: {error}"));
            }
        }));
    }

    fn add_grant(&self, path: &str, uri: &str) -> Result<(), String> {
        let mut grants = lock(&self.inner.grants);
        grants.insert(path.to_owned(), uri.to_owned());
        let bytes = serde_json::to_vec(&*grants).map_err(|e| format!("encode SAF URI grants: {e}"))?;
        photocraft_format::atomic_write(&self.inner.grant_file, &bytes)
            .map_err(|e| format!("persist SAF URI grants: {e}"))
    }

    fn uri_for_path(&self, path: &Path) -> Option<String> {
        lock(&self.inner.grants).get(path.to_str()?).cloned()
    }

    pub(super) fn sync_to_provider(&self, path: &Path) -> Result<(), String> {
        self.sync_to_provider_inner(path, None)
    }

    pub(super) fn sync_to_provider_with_ctl(&self, path: &Path, ctl: &SaveCtl) -> Result<(), String> {
        self.sync_to_provider_inner(path, Some(ctl))
    }

    fn sync_to_provider_inner(&self, path: &Path, ctl: Option<&SaveCtl>) -> Result<(), String> {
        let Some(uri) = self.uri_for_path(path) else { return Ok(()) };
        let path = path.to_str().ok_or("SAF working path is not valid Unicode")?.to_owned();
        let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(requests()).insert(request_id, PendingRequest::Write(tx));
        let payload = serde_json::json!({ "kind": "write", "uri": uri, "path": path });
        self.post_to_activity(request_id, WRITE_METHOD, payload.to_string());
        let deadline = Instant::now() + WRITE_TIMEOUT;
        loop {
            if let Some(ctl) = ctl { ctl.check()?; }
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Disconnected) => return Err("Android provider write callback disconnected".into()),
                Err(RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                    lock(requests()).remove(&request_id);
                    return Err("Android provider did not finish writing within 10 minutes".into());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

fn mime_for_extension(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "psd" | "psb" | "psdt" => "image/vnd.adobe.photoshop",
        "pcraft" => "application/vnd.photocraft.document",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        "heic" | "heif" => "image/heif",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" | "svgz" => "image/svg+xml",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn complete_error(request_id: u64, message: String) {
    match lock(requests()).remove(&request_id) {
        Some(PendingRequest::Dialog { reply, .. }) => reply.send(Some(FileDialogAnswer::Unavailable(message))),
        Some(PendingRequest::Write(tx)) => { let _ = tx.send(Err(message)); }
        None => log::warn!("Ignoring late Android SAF error callback for request {request_id}"),
    }
}

fn complete_result(request_id: u64, payload: &str) {
    let value: serde_json::Value = match serde_json::from_str(payload) {
        Ok(value) => value,
        Err(error) => { complete_error(request_id, format!("Invalid Android SAF response: {error}")); return; }
    };
    match lock(requests()).remove(&request_id) {
        Some(PendingRequest::Dialog { reply, inner }) => match value.get("kind").and_then(serde_json::Value::as_str) {
            Some("cancel") => reply.send(None),
            Some("error") => reply.send(Some(FileDialogAnswer::Unavailable(
                value.get("message").and_then(serde_json::Value::as_str).unwrap_or("Android document picker failed").to_owned(),
            ))),
            Some("open") => {
                let mut paths = Vec::new();
                let mut error = None;
                for file in value.get("files").and_then(serde_json::Value::as_array).into_iter().flatten() {
                    let Some(path) = file.get("path").and_then(serde_json::Value::as_str) else {
                        error = Some("Android SAF returned a file without a local path".to_owned()); break;
                    };
                    let Some(uri) = file.get("uri").and_then(serde_json::Value::as_str) else {
                        error = Some("Android SAF returned a file without a provider URI".to_owned()); break;
                    };
                    let bridge = SafBridge { inner: inner.clone() };
                    if let Err(e) = bridge.add_grant(path, uri) { error = Some(e); break; }
                    paths.push(path.to_owned());
                }
                if let Some(error) = error { reply.send(Some(FileDialogAnswer::Unavailable(error))); }
                else if paths.is_empty() { reply.send(None); }
                else { reply.send(Some(FileDialogAnswer::Paths(paths))); }
            }
            Some("save") => {
                let path = value.get("path").and_then(serde_json::Value::as_str);
                let uri = value.get("uri").and_then(serde_json::Value::as_str);
                match (path, uri) {
                    (Some(path), Some(uri)) => match (SafBridge { inner }).add_grant(path, uri) {
                        Ok(()) => reply.send(Some(FileDialogAnswer::SaveTo(path.to_owned()))),
                        Err(error) => reply.send(Some(FileDialogAnswer::Unavailable(error))),
                    },
                    _ => reply.send(Some(FileDialogAnswer::Unavailable("Android SAF returned an invalid save target".into()))),
                }
            }
            _ => reply.send(Some(FileDialogAnswer::Unavailable("Android SAF returned an unknown picker result".into()))),
        },
        Some(PendingRequest::Write(tx)) => {
            let result = if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
                Ok(())
            } else {
                Err(value.get("message").and_then(serde_json::Value::as_str).unwrap_or("Android could not write the selected document").to_owned())
            };
            let _ = tx.send(result);
        }
        None => log::warn!("Ignoring late Android SAF callback for request {request_id}"),
    }
}

/// Native callback for picker completion and provider-write completion.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_storyteller_photocraft_PhotoCraftActivity_nativeOnSafResult(
    mut raw_env: jni::JNIEnv<'_>,
    _activity: jobject,
    request_id: jlong,
    payload: jstring,
) {
    let outcome = raw_env.with_env(|env| -> jni::errors::Result<()> {
        let payload_object = payload as jobject;
        // SAFETY: JNI passes a live local jstring for this native call; the cast does not own it.
        let payload_ref = unsafe { env.as_cast_raw::<JString>(&payload_object)? };
        let payload = payload_ref.try_to_string(env)?;
        complete_result(request_id as u64, &payload);
        Ok(())
    });
    let _ = outcome.resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}

#[cfg(test)]
mod tests {
    use super::mime_for_extension;

    #[test]
    fn mime_mapping_covers_native_and_flat_image_documents() {
        assert_eq!(mime_for_extension("PSD"), "image/vnd.adobe.photoshop");
        assert_eq!(mime_for_extension("pcraft"), "application/vnd.photocraft.document");
        assert_eq!(mime_for_extension("png"), "image/png");
        assert_eq!(mime_for_extension("unknown"), "application/octet-stream");
    }
}
