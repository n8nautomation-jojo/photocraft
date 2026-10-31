//! GameActivity entrypoint. Android-specific symbol export is isolated in this module.

#![allow(unsafe_code)]

use eframe::{NativeOptions, Renderer};
use photocraft_engine::Session;
use photocraft_ui_egui::{PhotocraftApp, Services};
use winit::platform::android::activity::AndroidApp;

// SAFETY: android-activity/winit requires exactly one android_main symbol in this cdylib.
// The symbol uses the Rust ABI and the AndroidApp type re-exported by this workspace's pinned
// winit dependency. No other unsafe operation is performed in this module.
#[unsafe(no_mangle)]
pub fn android_main(android_app: AndroidApp) {
    let options = NativeOptions {
        renderer: Renderer::Wgpu,
        android_app: Some(android_app),
        ..Default::default()
    };

    let result = eframe::run_native(
        "PhotoCraft",
        options,
        Box::new(|creation| {
            PhotocraftApp::install_type_reset_hooks(&creation.egui_ctx);
            Ok(Box::new(PhotocraftApp::new(Session::new(), Services::default())))
        }),
    );

    if let Err(error) = result {
        eprintln!("PhotoCraft Android runtime failed: {error}");
    }
}
