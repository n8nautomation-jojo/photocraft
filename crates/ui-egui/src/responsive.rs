//! Viewport-driven compact layouts shared by the native UI shell.
//!
//! Breakpoints are measured in egui points (Android dp after the platform scale), not physical
//! pixels or a guessed device model. This lets resizable windows and rotated phones adapt too.

/// Below this width, a side toolbar and side dock leave too little room for the document canvas.
pub(crate) const COMPACT_WIDTH: f32 = 720.0;
/// A short landscape viewport needs the same bottom-sheet panel layout even when it is wide.
pub(crate) const COMPACT_HEIGHT: f32 = 460.0;

/// Whether the current content viewport should use the phone-first compact workspace.
pub(crate) fn compact(ctx: &egui::Context) -> bool {
    let size = ctx.content_rect().size();
    size.x.is_finite() && size.y.is_finite() && (size.x < COMPACT_WIDTH || size.y < COMPACT_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2, Context, RawInput, Rect};

    fn compact_at(size: egui::Vec2) -> bool {
        let ctx = Context::default();
        let input = RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            assert_eq!(compact(ui.ctx()), size.x < COMPACT_WIDTH || size.y < COMPACT_HEIGHT);
        });
        out.textures_delta.clear();
        compact(&ctx)
    }

    #[test]
    fn phone_portrait_and_landscape_use_compact_workspace() {
        assert!(compact_at(vec2(393.0, 852.0)));
        assert!(compact_at(vec2(852.0, 393.0)));
        assert!(compact_at(vec2(640.0, 900.0)));
    }

    #[test]
    fn tablet_and_desktop_keep_the_full_workspace() {
        assert!(!compact_at(vec2(800.0, 1024.0)));
        assert!(!compact_at(vec2(1024.0, 768.0)));
        assert!(!compact_at(vec2(1440.0, 900.0)));
    }
}
