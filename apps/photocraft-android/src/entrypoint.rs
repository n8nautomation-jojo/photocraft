//! GameActivity entrypoint. Android-specific symbol export is isolated in this module.

#![allow(unsafe_code)]

use eframe::{NativeOptions, Renderer};
use photocraft_engine::Session;
use photocraft_ui_egui::{PhotocraftApp, theme::ThemeKind};
use winit::platform::android::activity::AndroidApp;

// SAFETY: android-activity/winit requires exactly one android_main symbol in this cdylib.
// The symbol uses the Rust ABI and the AndroidApp type re-exported by this workspace's pinned
// winit dependency. No other unsafe operation is performed in this module.
#[unsafe(no_mangle)]
pub fn android_main(android_app: AndroidApp) {
    let Some(data_root) = android_app.internal_data_path() else {
        log::error!("Android did not provide an internal application data directory");
        return;
    };

    let saf = match super::saf::SafBridge::new(android_app.clone(), &data_root.join("PhotoCraft")) {
        Ok(saf) => saf,
        Err(error) => {
            log::error!("Could not initialize Android document access: {error}");
            return;
        }
    };

    let lifecycle_events = super::lifecycle::LifecycleEvents::new();
    let services = match super::services::create(data_root, saf, lifecycle_events.clone()) {
        Ok(services) => services,
        Err(error) => {
            log::error!("Could not initialize PhotoCraft Android storage: {error}");
            return;
        }
    };

    // The egui context schedules a real redraw; the waker only interrupts Android's looper.
    let lifecycle_for_app = lifecycle_events.clone();
    super::lifecycle::install(android_app.create_waker(), lifecycle_events);

    let options = NativeOptions {
        renderer: Renderer::Wgpu,
        android_app: Some(android_app),
        ..Default::default()
    };

    let result = eframe::run_native(
        "PhotoCraft",
        options,
        Box::new(move |creation| {
            PhotocraftApp::setup_context(&creation.egui_ctx, ThemeKind::ProMedium);
            lifecycle_for_app.set_context(creation.egui_ctx.clone());
            Ok(Box::new(PhotocraftApp::new(Session::new(), services)))
        }),
    );

    super::lifecycle::uninstall();

    if let Err(error) = result {
        log::error!("PhotoCraft Android runtime failed: {error}");
    }
}
