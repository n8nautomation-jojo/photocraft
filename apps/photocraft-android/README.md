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
state stays under Android's internal data directory.

This is still a bootstrap, not a finished Android port. Android Storage Access Framework (SAF)
Open/Save dialogs and Android clipboard are not connected yet, so users cannot currently browse
arbitrary documents from inside the editor. The app deliberately does not pretend that a SAF
content URI is a normal filesystem path. Lifecycle-specific pause/low-memory recovery handling,
tablet/phone layout adaptation and device validation remain open tasks.

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
2. Add a SAF bridge for Open/Save and verify a PNG can be opened and saved through Android document
   providers without copying or corrupting user files.
3. Add the Android clipboard bridge and test text, pixel and image clipboard round trips.
4. Wire pause, stop, low-memory and device-loss lifecycle signals to recovery flushing, and test
   force-stop/process death and app switching.
5. Measure native memory with a 12 MP image and 10 layers on a real Android device; validate the
   configured GPU budget against actual device RAM.
6. Verify mouse primary/secondary, hover, stylus, keyboard shortcuts, Space+drag and wheel input on
   a tablet and a phone. Adapt the existing panels for tablet landscape and compact phone layouts.
