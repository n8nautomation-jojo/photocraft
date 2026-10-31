# PhotoCraft for Android

This project is the Android host for the existing PhotoCraft Rust editor. It keeps the existing
engine, command system, egui UI and wgpu renderer; Kotlin is limited to the Android GameActivity
entry class and future Android platform bridges.

## Current implementation boundary

The first bootstrap slice starts the existing PhotoCraftApp through eframe's GameActivity
integration and packages the Rust dynamic library for **arm64-v8a**.

This is not yet a feature-complete Android port. In particular, Android Storage Access Framework
Open/Save, Android clipboard and lifecycle-driven recovery wiring are not connected in this slice.
The host currently constructs Services::default(), so desktop filesystem dialogs and desktop
clipboard services are not pulled into Android. These are explicit next implementation tasks, not
silently substituted desktop implementations.

## Toolchain

Install:

- JDK 17
- Android SDK Platform 36 and Android Build Tools 36.0.0
- Android NDK 28.2.13676358
- Rust toolchain 1.95.0 (the version required by the root workspace)
- Rust target aarch64-linux-android
- cargo-ndk
- Gradle 9.5.1 or a compatible Android Studio Gradle installation

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
   disabling editor crates or codecs.
2. Add a SAF bridge for Open/Save and verify a PNG can be opened in the existing canvas.
3. Measure native memory with a 12 MP image and 10 layers on a real Android device.
4. Verify pointer, stylus, keyboard and wheel behavior on a tablet and a phone.
5. Wire Android lifecycle callbacks to autosave flush/recovery and test process death.
