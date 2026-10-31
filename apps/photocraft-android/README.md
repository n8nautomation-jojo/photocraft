# PhotoCraft for Android

This project is the Android host for the existing PhotoCraft Rust editor. It keeps the existing
engine, command system, egui UI and wgpu renderer; Kotlin is limited to the Android GameActivity
entry class and future Android platform bridges.

## Current implementation boundary

The first bootstrap slice starts the existing PhotoCraftApp through eframe's GameActivity
integration and packages the Rust dynamic library for **arm64-v8a**.

The Android host now initializes app-private preferences, the preset store, crash-recovery
autosaves, recovery discovery/adoption, codec import/export callbacks and atomic file writes.
These services use the existing PhotoCraft Rust engine and format implementation. App-private
state stays under Android's internal data directory. The existing GPU canvas budget now reads
Android's physical RAM from `/proc/meminfo` instead of falling back to the desktop's 16 GB
assumption. Desktop self-update UI and desktop-only eframe clipboard/floating-window features are
not included in the Android dependency target.

This RAM detection is not the requested global 40% tile-memory manager or reconstructible-tile LRU;
those remain separate engineering gates.

The Android host now has a first SAF Open/Save bridge:
- Open supports one or multiple documents through ACTION_OPEN_DOCUMENT.
- Selected documents are streamed into persistent app-private working files, avoiding whole-file
  buffering and preserving the source document.
- Persistable URI grants are recorded in app-private storage, so Save can stream changes back to
  the selected document provider after the local atomic save succeeds.
- Save As uses ACTION_CREATE_DOCUMENT and writes through ContentResolver on a background worker.
- Content URIs are never passed to PhotoCraft as if they were ordinary filesystem paths.

The Android Activity now forwards pause/resume transitions to the Rust event loop. On pause, the
Rust UI loop requests a recovery snapshot of every dirty document through the existing
RecoveryStore/flush path; the Activity callback only queues the event and wakes the native looper,
so it does not perform encoding or filesystem work on Android's UI thread. Rust requests an
egui repaint through eframe's registered repaint callback (the looper wake alone is not a redraw).
Resume requests a fresh egui frame. This is a best-effort checkpoint request, not a guarantee against immediate process
termination: it still needs real-device verification. On Android `onTrimMemory` running-low and
more severe callbacks, PhotoCraft checkpoints dirty documents and drops rebuildable canvas,
thumbnail, histogram, LUT and GPU-compositor caches. The CPU document surfaces, undo history and
active editing state are not evicted by this first memory-pressure response.

The Android host now has a phone-first responsive workspace while keeping the existing Rust
editor, command system, and wgpu canvas:

- Layout selection uses the live egui content viewport, not a device-brand/model check. A viewport
  narrower than 720 points or shorter than 460 points uses the compact workspace, so portrait and
  short-landscape phones both adapt, including resizable windows.
- The desktop vertical toolbar and narrow side dock are replaced by a horizontal touch tool shelf
  with 48-point targets, a horizontally scrollable set of common tools, a name-labelled
  "More" menu for every non-hidden toolbar tool, a Panels toggle, zoom percentage and Fit action.
- The ten desktop top-level menus collapse into one 48-point Menu launcher on compact viewports;
  each original menu and its nested commands remain available inside a vertically scrollable menu.
- Panels are presented in a full-width bottom drawer. Its panel selector scrolls horizontally and
  can switch between Layers, Channels, Paths, Properties, Adjustments, Color, Swatches, Brushes,
  History, Actions, Navigator, Histogram, Info, Character, Paragraph, Gradients, Patterns and
  Layer Comps. Closing the drawer returns the space to the canvas.
- Portrait starts with the drawer open when the dock preference is enabled; short landscape starts
  with it closed to prioritize canvas height. The selection and open/closed state are temporary
  presentation state. The saved desktop/tablet workspace tree is not rewritten by phone rotation
  or by opening/closing the drawer.
- Wide tablet and desktop viewports retain the existing vertical toolbar, side dock, and workspace
  docking behavior. The responsive breakpoints and phone drawer behavior have unit/UI tests in the
  repository, but those tests have not yet been executed in an Android build environment.

The folder picker and Android clipboard are not connected yet. The SAF and lifecycle bridges have
not been compiled or tested on a device. Provider permission policies differ, so an unavailable
persistent grant or failed provider write is reported as a save error rather than silently marking
the document saved. Thermal/device-loss handling and process-death/force-stop validation remain
open tasks; touch-target verification and visual tuning on real phones and tablets are also pending.

## Toolchain

Install:

- JDK 17 (AGP 9.x built-in Kotlin support; no separate Kotlin Android plugin is applied)
- Android SDK Platform 36 and Android Build Tools 36.0.0
- Android NDK 28.2.13676358
- Rust toolchain 1.95.0 (the version required by the root workspace)
- Rust target aarch64-linux-android
- cargo-ndk
- Gradle 9.5.1 or newer within the AGP 9.3.1 compatibility range

Set ANDROID_SDK_ROOT and ANDROID_NDK_HOME to the installed SDK and NDK directories. For example,
on a Linux host:

```sh
rustup toolchain install 1.95.0 --profile minimal
rustup target add --toolchain 1.95.0 aarch64-linux-android
cargo +1.95.0 install cargo-ndk --locked
export ANDROID_SDK_ROOT="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/28.2.13676358"
```

Open apps/photocraft-android/android in Android Studio, or run the Gradle :app:assembleDebug task
from that directory. The Gradle preBuild task invokes cargo ndk from the repository root and
places the generated libmain.so under the app's local jniLibs/arm64-v8a directory.

For a direct native compile before packaging:

```sh
cargo +1.95.0 ndk -t arm64-v8a -o apps/photocraft-android/android/app/src/main/jniLibs build --release --locked -p photocraft-android --lib
```

The generated native library, Gradle caches, SDK settings and build outputs are intentionally
ignored by Git. They must remain local build artifacts.

## Next gates

1. Compile the Android package and resolve every actual target-specific compile/link error without
   disabling editor crates or codecs. This target has not yet been built in a real Android toolchain.
2. Compile and test SAF Open/Save against Android's system provider and at least one cloud-backed
   document provider, including cancellation, multi-select, persisted grants and write failures.
3. Add the Android folder picker and clipboard bridge and test image/text clipboard round trips.
4. Validate pause/resume recovery on a real device, then add stop, low-memory, thermal and
   device-loss handling plus process-death/force-stop and app-switching tests.
5. Measure native memory with a 12 MP image and 10 layers on a real Android device; validate the
   configured GPU budget against actual device RAM.
6. Validate the compact toolbar, panel drawer and touch targets on real phones in portrait and
   landscape, then verify mouse primary/secondary, hover, stylus, keyboard shortcuts, Space+drag
   and wheel input on both phones and tablets. Tune the tablet breakpoints from those measurements.
