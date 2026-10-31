//! Android Activity lifecycle signals shared with the egui frame loop.
//!
//! The Kotlin host only forwards lifecycle transitions. Document access and recovery remain in
//! Rust, and the UI thread consumes the queue at the normal per-frame OS-event boundary.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use photocraft_ui_egui::{OsEvent, OsEventsFn};
use winit::platform::android::activity::AndroidAppWaker;

const EVENT_PAUSED: i32 = 1;
const EVENT_RESUMED: i32 = 2;

#[derive(Clone)]
pub(super) struct LifecycleEvents {
    queue: Arc<Mutex<VecDeque<OsEvent>>>,
    repaint: Arc<Mutex<Option<eframe::egui::Context>>>,
}

impl LifecycleEvents {
    pub(super) fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            repaint: Arc::new(Mutex::new(None)),
        }
    }

    /// Attach the egui context after eframe creates the native application.
    pub(super) fn set_context(&self, context: eframe::egui::Context) {
        *lock(&self.repaint) = Some(context);
    }

    pub(super) fn os_events(&self) -> OsEventsFn {
        let queue = Arc::clone(&self.queue);
        Box::new(move || lock(&queue).drain(..).collect())
    }
}

struct ActiveBridge {
    events: LifecycleEvents,
    waker: AndroidAppWaker,
    paused: bool,
}

static ACTIVE: OnceLock<Mutex<Option<ActiveBridge>>> = OnceLock::new();

fn active() -> &'static Mutex<Option<ActiveBridge>> {
    ACTIVE.get_or_init(|| Mutex::new(None))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Publish the event queue and a waker for the current Activity's native event loop.
pub(super) fn install(waker: AndroidAppWaker, events: LifecycleEvents) {
    *lock(active()) = Some(ActiveBridge { events, waker, paused: false });
}

/// Remove the callback target before the Activity's native entrypoint returns.
pub(super) fn uninstall() {
    *lock(active()) = None;
}

/// Called by the JNI boundary in `saf.rs`. Unknown values and redundant transitions are ignored.
pub(super) fn dispatch(event: i32) {
    let (wake, repaint) = {
        let mut slot = lock(active());
        let Some(bridge) = slot.as_mut() else { return };
        let (paused, os_event) = match event {
            EVENT_PAUSED => (true, OsEvent::Suspend),
            EVENT_RESUMED => (false, OsEvent::Resume),
            _ => return,
        };
        if bridge.paused == paused {
            return;
        }
        bridge.paused = paused;
        lock(&bridge.events.queue).push_back(os_event);
        (bridge.waker.clone(), lock(&bridge.events.repaint).clone())
    };

    // AndroidAppWaker interrupts the native looper but does not itself schedule a winit redraw.
    // egui's repaint callback is what asks eframe/winit to issue the frame that drains os_events.
    if let Some(context) = repaint {
        context.request_repaint();
    }
    wake.wake();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_event_queue_drains_each_signal_once() {
        let events = LifecycleEvents::new();
        lock(&events.queue).push_back(OsEvent::Suspend);
        lock(&events.queue).push_back(OsEvent::Resume);
        let mut poll = events.os_events();
        assert_eq!(poll(), vec![OsEvent::Suspend, OsEvent::Resume]);
        assert!(poll().is_empty());
    }
}
