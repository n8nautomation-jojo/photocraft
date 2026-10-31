//! Android platform services backed by the app's private internal storage.
//!
//! Files in the app-private area are used for preferences, presets and crash recovery. User
//! documents must be accessed through the Storage Access Framework; this module deliberately does
//! not pretend that Android content URIs are ordinary filesystem paths. SAF dialogs are a later
//! bridge and are not installed until that bridge can provide real read/write capabilities.

use photocraft_codecs::{ChannelLayout, EncodeOptions, Image};
use photocraft_doc::Document;
use photocraft_format::RecoveryStore;
use photocraft_ui_egui::{
    ExportSettings, FileStamp, Recoverable, Services,
    jobs_ui::SaveCtl,
};
use std::cell::{RefCell};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

const ENCODE_SHARE: f32 = 0.85;

type SharedRecovery = Rc<RefCell<Option<RecoveryStore>>>;

fn with_store<R>(store: &SharedRecovery, f: impl FnOnce(&mut RecoveryStore) -> R) -> Result<R, String> {
    let mut slot = store.try_borrow_mut().map_err(|_| "crash recovery is busy".to_string())?;
    Ok(f(slot.as_mut().ok_or("Android recovery storage is unavailable")?))
}

fn recovery_services(dir: PathBuf) -> Services {
    let recover_dir = dir.clone();
    let store: SharedRecovery = Rc::new(RefCell::new(Some(RecoveryStore::new(dir))));
    let s1 = store.clone();
    let s2 = store.clone();
    let s3 = store.clone();
    let s4 = store.clone();

    Services {
        autosave: Some(Box::new(move |doc: &Arc<Document>, revision: u64, path: Option<&str>| {
            with_store(&s1, |store| store.autosave_checked(doc, revision, path.map(str::to_owned)))?
        })),
        autosave_results: Some(Box::new(move || with_store(&s2, RecoveryStore::take_completed).unwrap_or_default())),
        flush_autosaves: Some(Box::new(move || with_store(&s3, RecoveryStore::flush).unwrap_or_default())),
        discard_autosave: Some(Box::new(move |id: u64| {
            if let Err(error) = with_store(&s4, |store| store.discard(id)) {
                log::warn!("Could not discard Android recovery entry for document {id}: {error}");
            }
        })),
        recover: Some(Box::new(move || {
            photocraft_format::list_recovery(&recover_dir)
                .into_iter()
                .map(|entry| Recoverable {
                    key: entry.info.key.clone(),
                    name: entry.info.document_name.clone(),
                    path: entry.info.original_path.clone(),
                    load: Box::new(move || photocraft_format::recover(&entry).map_err(|error| error.to_string())),
                })
                .collect()
        })),
        adopt_autosave: Some(Box::new(move |id: u64, key: &str| {
            if let Err(error) = with_store(&store, |store| store.adopt(id, key)) {
                log::warn!("Could not adopt Android recovery entry for document {id}: {error}");
            }
        })),
        ..Default::default()
    }
}

fn export_file(
    doc: &Document,
    path: &str,
    settings: &ExportSettings,
    ctl: &SaveCtl,
) -> Result<Vec<String>, String> {
    ctl.progress(0.0, "Encoding");
    let options = settings.io_options();
    let cancelled = || ctl.check().is_err();
    let progress = |fraction: f32| ctl.progress(fraction * ENCODE_SHARE, "");
    let interrupt = photocraft_raster::Interrupt::new(&cancelled, &progress);
    let result = photocraft_io::export_with(doc, path, &options, &interrupt).map_err(|error| error.to_string())?;

    // Commit before the first filesystem mutation. A cancelled save never replaces its target.
    ctl.commit()?;
    ctl.progress(ENCODE_SHARE, "Writing");
    photocraft_format::atomic_write_progress(Path::new(path), &result.bytes, &|fraction| {
        ctl.progress(ENCODE_SHARE + (0.99 - ENCODE_SHARE) * fraction, "");
    })
    .map_err(|error| error.to_string())?;
    ctl.progress(1.0, "");
    Ok(result.warnings)
}

/// Construct platform services after the GameActivity has supplied its private data directory.
pub(super) fn create(internal_data_dir: PathBuf) -> Result<Services, String> {
    let app_data = internal_data_dir.join("PhotoCraft");
    let recovery_dir = app_data.join("Recovery");
    let presets_dir = app_data.join("Presets");
    std::fs::create_dir_all(&recovery_dir).map_err(|error| format!("create recovery directory: {error}"))?;
    std::fs::create_dir_all(&presets_dir).map_err(|error| format!("create presets directory: {error}"))?;

    let prefs_file = app_data.join("preferences.json");
    let recovery = recovery_services(recovery_dir);

    Ok(Services {
        import: Some(Box::new(|name: &str, bytes: &[u8], options: &photocraft_io::ImportOptions| {
            photocraft_io::import_with_options(name, bytes, options)
                .map(|result| (result.document, result.warnings))
                .map_err(|error| error.to_string())
        })),
        export: Some(Box::new(|doc: &Document, path: &str, settings: &ExportSettings| {
            photocraft_io::export(doc, path, &settings.io_options())
                .map(|result| (result.bytes, result.warnings))
                .map_err(|error| error.to_string())
        })),
        save_file: Some(Arc::new(export_file)),
        write: Some(Box::new(|path: &str, bytes: &[u8]| {
            photocraft_format::atomic_write(Path::new(path), bytes).map_err(|error| error.to_string())
        })),
        file_stamp: Some(Box::new(|path: &str| {
            let metadata = std::fs::metadata(path).ok()?;
            Some(FileStamp { len: metadata.len(), modified: metadata.modified().ok() })
        })),
        load_prefs: Some(Box::new({
            let path = prefs_file.clone();
            move || std::fs::read_to_string(&path).ok()
        })),
        save_prefs: Some(Box::new({
            let path = prefs_file.clone();
            move |text: &str| photocraft_format::atomic_write(&path, text.as_bytes()).map_err(|error| error.to_string())
        })),
        encode_png: Some(Box::new(|width, height, rgba| {
            let image = Image::from_u8(width, height, ChannelLayout::Rgba, rgba.to_vec()).map_err(|error| error.to_string())?;
            photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &EncodeOptions::default())
                .map_err(|error| error.to_string())
        })),
        append_text: Some(Box::new(|path: &str, text: &str| {
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|error| error.to_string())?;
            file.write_all(text.as_bytes()).map_err(|error| error.to_string())
        })),
        preset_store: Some(photocraft_engine::preset_store::open_dir_async(presets_dir)),
        ..recovery
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_storage_layout_is_scoped_under_the_android_app_data_directory() {
        let root = PathBuf::from("/data/user/0/ai.storyteller.photocraft/files");
        let app_data = root.join("PhotoCraft");
        assert_eq!(app_data.join("preferences.json"), root.join("PhotoCraft/preferences.json"));
        assert_eq!(app_data.join("Recovery"), root.join("PhotoCraft/Recovery"));
        assert_eq!(app_data.join("Presets"), root.join("PhotoCraft/Presets"));
    }
}
