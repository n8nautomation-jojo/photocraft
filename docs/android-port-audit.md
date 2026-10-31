# PhotoCraft Android Port — Baseline Audit

**Audit date:** 2026-10-31  
**Repository:** `n8nautomation-jojo/photocraft`  
**Audited branch:** `main`  
**Audited commit:** `422ca7beda90d927643bcd074646ebbd37706d75`  
**Upstream:** `storytold/photocraft`  
**Scope:** G0 repository and architecture audit before Android implementation

## Executive decision

**G0 static audit: complete. G1: BLOCKED — build environment unavailable.**

The fork's `main` was identical to upstream `main` at the audited commit. The repository contains the Rust PhotoCraft workspace and its existing egui/eframe/wgpu application, but it does not contain an Android application module, Gradle project, Android manifest, GameActivity integration, or Android-specific platform-services implementation.

No application code, dependency manifest, lockfile, license, attribution, or upstream branch was changed during this audit. This document is the only file added by this audit branch.

G1 has **not** been reported as a source-code failure: it could not be executed in the available environment. No later gate may begin until G1 has been run and passed with actual Android toolchain output.

## 1. Repository integrity and Git state

| Check | Result |
|---|---|
| Repository | `n8nautomation-jojo/photocraft` |
| Repository is a fork | Yes; parent/source is `storytold/photocraft` |
| Default branch | `main` |
| Audited main SHA | `422ca7beda90d927643bcd074646ebbd37706d75` |
| Upstream main SHA at audit | Same SHA |
| Push/admin permission | Available to the linked repository account |
| Existing Android project | Not found in the recursive repository tree |
| Existing open PRs in the fork at audit | None |
| Required legal files | `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `ATTRIBUTION.md` present |

The legal and attribution files must remain intact in all subsequent work. Any new assets must meet the project's licensing rules and be recorded in `ATTRIBUTION.md`.

## 2. Architecture and integration findings

### 2.1 Existing application and UI seam

The current desktop application is `apps/photocraft`. Its entry point uses `eframe::run_native` and constructs `PhotocraftApp` with a `Session` and injected `photocraft_ui_egui::Services`.

This is a valuable seam for the Android port: the egui UI and engine should remain shared, while Android supplies platform services through the existing injection points. The Android host should not duplicate the command registry, tool state machines, document model, or image-processing engine.

Relevant locations:

- `apps/photocraft/src/main.rs`
- `apps/photocraft/src/services.rs`
- `crates/ui-egui/src/services.rs`
- `crates/ui-egui/src/file_dialog.rs`
- `crates/engine`
- `crates/doc`, `crates/raster`, `crates/compose`, `crates/gpu`

### 2.2 Android entry point is not configured

The workspace pins eframe 0.37 and winit 0.30.13. The current `apps/photocraft/Cargo.toml` enables eframe's `wgpu`, `accesskit`, `wayland`, `x11`, `persistence`, and `clipboard` features, but does not enable the Android GameActivity backend.

The current desktop `main.rs` is a desktop-oriented startup path: it parses desktop launch arguments, initializes desktop services, performs window/display setup, and invokes `eframe::run_native`. There is no Android-specific entry point or host integration in the audited tree.

The eframe 0.37 feature set documents `android-game-activity` through egui-winit. The Android glue version must be kept compatible with the winit version actually resolved by this workspace; do not independently add an arbitrary `android-activity` version. Confirm the resolved dependency graph with `cargo tree` / `cargo metadata` before editing.

References:
- [eframe 0.37 features](https://docs.rs/crate/eframe/0.37.0/source/Cargo.toml)
- [winit Android platform notes](https://docs.rs/winit/0.30.13/winit/platform/android/)
- [Android GameActivity documentation](https://developer.android.com/games/agdk/game-activity)

### 2.3 Desktop dependencies must be separated, not stubbed

The app manifest currently groups several dependencies under `cfg(not(target_arch = "wasm32"))`, which also matches Android. This includes desktop-oriented integrations such as `rfd`, `arboard`, and `open`. Separate target-specific dependencies also exist for Linux, Windows, and macOS.

The Android port must review the *resolved* dependency graph and move or gate only the integrations that are truly platform-specific. Do not disable image formats, codecs, or engine capabilities to make a cross-compile pass.

The app's large native service implementation is path/filesystem based. Android SAF exposes document URIs and content streams, not ordinary desktop filesystem paths. The host must bridge SAF access to the existing import/export and save services without changing the engine command system. Use app-private cache/files directories for temporary working copies and recovery data; persist user-facing document access through Android's URI permission model.

Potential integration points:

- `photocraft_ui_egui::FileDialogFn` and `FileDialogRequest`
- injected import/export/save callbacks in `photocraft_ui_egui::Services`
- `apps/photocraft/src/services.rs` as the desktop reference, not as an Android implementation

### 2.4 Required Android device feature

The Android manifest must include:

```xml
<uses-feature
    android:name="android.hardware.type.pc"
    android:required="false" />
```

This must be verified in the merged manifest of the actual APK/AAB, not merely in a source manifest. Mouse secondary-button, hover, wheel, and keyboard delivery still require physical-device verification.

### 2.5 Memory-management gap: high risk on mobile

The existing GPU canvas has a GPU-memory budget in `crates/ui-egui/src/canvas/gpu_canvas/budget.rs`. It budgets canvas textures and GPU layer/effect resources; it is **not** a global process memory manager or an LRU tile-store implementation.

Important current behavior:

- `physical_memory()` caches a physical-RAM detection result.
- Detection is implemented for macOS, Linux, FreeBSD, and Windows.
- Android falls through the generic unsupported-platform branch and returns `None`.
- The GPU budget helpers use a 16 GiB fallback when RAM is unknown, then derive a quarter-RAM budget (subject to their other limits).

That fallback is inappropriate as an Android device-memory assumption. Before Android rendering is exposed, the Android path must supply a trustworthy device-memory value or use a conservative, explicitly documented fallback. Do not treat the existing GPU canvas budget as satisfying the required 40%-of-device-RAM global tile-memory budget.

The raster crate stores sparse 256×256 tiles in an in-memory `BTreeMap<TileCoord, Arc<Tile>>` with copy-on-write sharing. The audited source tree contains no implemented tile-store/LRU/scratch-spill module. The `ScratchDisks` preference exists, but the current scorecard lists `scratch_disks.disks` among settings that are not read. A preference field is not proof that scratch storage works.

Required future design work (not implemented in this audit):

- distinguish resident/reconstructible cache tiles from authoritative dirty document data;
- enforce a global budget across tracked CPU tile/cache allocations and tracked GPU allocations without double-counting shared memory;
- evict only tiles that can be safely reconstructed or restored;
- never evict the only recoverable copy of modified document data;
- use bounded, validated scratch storage for eligible data;
- avoid whole-layer allocations for layers above 8 MP;
- preserve Undo/Redo correctness and file fidelity.

### 2.6 Autosave and crash recovery already exist

The core native-format layer contains a background `Autosaver` and `RecoveryStore` in `crates/format/src/autosave.rs`. It supports incremental saves, queued background work, recovery listing/loading, adoption, flushing, and discarding.

The desktop app wires these facilities through its native services and config-directory implementation. Android must reuse this existing recovery architecture and supply safe app-private directories. It must also connect Android lifecycle signals to the existing asynchronous save path; do not perform long synchronous writes in `onPause` or assume `onStop` guarantees completion.

The presence of core autosave code does not prove Android lifecycle recovery. A force-stop/relaunch recovery test on a physical Android device is still required.

### 2.7 GPU failure handling is not live device recreation

The desktop application has GPU startup fallback and a GPU-health path. The documented device-loss handling flushes autosaves and exits so the next launch can select a safer backend. This is not evidence that a live wgpu device can be recreated in the same Android process.

The known P12 scorecard issue is a wgpu validation error involving `Queue::write_buffer` and the invalid `pc_compose_uniforms` buffer in a 150-layer scenario. That error must be diagnosed as a validation/resource-lifetime failure; it must not be described as proof of Vulkan device loss.

Android device recreation and CPU fallback remain unverified.

### 2.8 Input and responsive layout

No Android-specific input integration or responsive profile implementation was found. The existing tablet crate targets desktop platforms (macOS, Windows, Linux/X11); its documentation explicitly describes platform-specific pen support and limitations. Android stylus pressure/tilt must not be claimed until the Android event path is inspected and tested on supported hardware.

The requested dp profiles and 48dp touch-target coverage are not present as Android-specific features. Existing egui UI tests and screenshot infrastructure can help, but do not establish physical-device layout/input acceptance.

## 3. Existing reliability and performance evidence

The audited commit is identical to upstream `main` at the time of this audit. Upstream GitHub Actions has a CI run for this exact commit:

[Upstream CI run for `422ca7beda90d927643bcd074646ebbd37706d75`](https://github.com/storytold/photocraft/actions/runs/41171241825)

In that run, the Linux gate, corpus tests, Linux tests, Windows unit tests, Windows integration tests, and WebAssembly build/size gate completed successfully. Several opt-in performance/fuzz jobs were skipped by their workflow conditions; they must not be represented as executed.

The check-runs also reported a failed `SonarCloud Code Analysis` check even though the SonarCloud workflow's individual scan job reported success. The reason for that separate code-analysis check was not established here and must be investigated before treating the full check suite as clean.

This is **upstream CI evidence for the same source SHA**, not a new local test run on the fork and not Android evidence. The fork itself had no Actions runs at audit time.

The committed performance scorecard is not an Android baseline. Its recorded machine baseline is an Apple M4 Pro on macOS, and its own documentation describes that run as contended. P12 is a known failure; P25, P49, and P50 are listed as not run in the scorecard. None of those measurements prove Android performance or RSS.

## 4. Local execution blockers

The available execution environment for this task did not provide:

- Rust compiler / Cargo (workspace requires Rust 1.95);
- cargo-ndk;
- Android NDK;
- Android SDK / Gradle;
- adb or a connected Android test device;
- working outbound GitHub DNS/network for cloning and running the repository locally.

Therefore the following commands and tests were **not run** in this audit environment:

- `cargo metadata --locked`
- `cargo test --workspace`
- `cargo xtask ci`
- `cargo-ndk` cross-compilation for `aarch64-linux-android`
- Android Gradle build / APK packaging
- physical-device rendering, input, lifecycle, and memory tests

These are environment blockers, not evidence that G1 source compilation fails. No build result, RSS value, or device behavior is claimed.

## 5. Gate decision

| Gate | Status | Evidence / reason |
|---|---|---|
| G0 — repository and architecture audit | **PASS (static audit)** | Fork/source identity, manifests, app entry point, service seam, architecture, platform gaps, memory/recovery design, and upstream same-SHA CI evidence inspected. Local baseline commands could not be re-executed. |
| G1 — Android cross-compilation | **BLOCKED — not executed** | No Rust/Cargo, cargo-ndk, or Android NDK available in the execution environment. No source-level success/failure is asserted. |
| G2–G8 | **NOT STARTED** | Must not proceed until G1 is executed and passes. |

## 6. Required next actions before G1 can be accepted

1. Provide an execution runner or workstation with the repository, Rust 1.95 toolchain, cargo-ndk, and a supported Android NDK installed.
2. Record `cargo metadata --locked --format-version 1` and the actual package names for the requested crates.
3. Capture the pre-change host test baseline on that runner.
4. Cross-compile only the required crates for `aarch64-linux-android` using their real features and dependencies.
5. Report every error and resolve platform dependency problems without silently disabling codecs or engine functionality.
6. Stop after G1 and attach the complete logs. Do not start G2 until G1 passes.

## 7. Change summary

- Added: this audit document only.
- No Rust source, Cargo manifest, Cargo.lock, Android manifest, license, notice, or attribution file changed.
- No APK/AAB was produced.
- No PR was merged.


## 8. G1 follow-up — source-only manifest and toolchain review (2026-10-31)

This section was added after the G0 report was merged. It is a source inspection only; it does not replace an Android build.

### 8.1 Verified Cargo package names and direct dependency shape

The nine requested directories contain these actual Cargo package names:

| Requested crate | Cargo package | Direct workspace dependencies / relevant target dependencies |
|---|---|---|
| raster | `photocraft-raster` | geom, color; `rayon` on all non-WASM targets |
| color | `photocraft-color` | cms |
| geom | `photocraft-geom` | none |
| doc | `photocraft-doc` | geom, color, raster |
| compose | `photocraft-compose` | geom, color, raster, doc, vector; `rayon` on all non-WASM targets |
| psd | `photocraft-psd` | standalone; `thiserror`, `flate2`, optional `serde` |
| codecs | `photocraft-codecs` | image, png, tiff, exr, half, serde_json, flate2, WebP/JPEG/JXL codec crates; `rayon` on all non-WASM targets |
| paint | `photocraft-paint` | geom, color, raster; `rayon` on all non-WASM targets |
| text | `photocraft-text` | geom, color, raster, doc, psd, parley, skrifa, flate2; `rayon` and parley complex-script support on all non-WASM targets |

The transitive workspace closure therefore also includes at least `photocraft-cms` and `photocraft-vector`; a correct G1 run must let Cargo resolve and compile the full dependency graph, not just these nine manifests in isolation.

### 8.2 Source-level observations (not compile results)

- The inspected direct manifests do not declare Linux desktop system libraries, macOS frameworks, Windows APIs, or JNI dependencies for these nine requested packages.
- Several crates intentionally use `cfg(not(target_arch = "wasm32"))` for Rayon. Android satisfies that condition; the Android build will therefore compile the multithreaded native code path rather than the single-threaded WASM path.
- `photocraft-text` enables parley's `complex-scripts` on native non-WASM targets. Its special `system` font backend is enabled only for macOS; Android will use the shared font-loading path, which still requires physical-device validation with bundled and user-imported fonts.
- `photocraft-psd` is intentionally standalone, with no workspace dependencies.
- `photocraft-codecs` has no default optional features. Its `heif` feature adds the optional pure-Rust `photocraft-heif` decoder; the application-level `heif` feature forwards to it. Its separate `avif` feature enables AVIF encoding through the image crate. These features must be tested explicitly when they are part of the Android distribution configuration; they must not be silently removed to make a build pass.
- The workspace pins `flate2` with the `zlib-rs` backend, not a system zlib dependency.
- No Android target build, target-specific Cargo resolution, or native linker invocation was possible. Thus none of the above proves compatibility; transitive build scripts and target-specific code may still fail.

### 8.3 Verified eframe/winit Android compatibility contract

The current lockfile resolves:

- `eframe 0.37.0` from the workspace's pinned egui git revision;
- `winit 0.30.13` from the workspace's pinned winit git revision;
- `android-activity 0.6.1` in winit's dependency graph.

The eframe 0.37 feature catalogue exposes `android-game-activity` by forwarding to `egui-winit/android-game-activity`. The pinned winit 0.30 line requires android-activity 0.6. Winit documents that the glue crate version must match and recommends using its re-exported activity API rather than adding a second direct version of `android-activity`.

Implications for the implementation phase:

1. Enable the eframe `android-game-activity` feature only in the Android build configuration.
2. Do not add an independent incompatible `android-activity` dependency.
3. Inspect the pinned egui/winit source revision before writing the Android entry point; public latest docs are useful for the contract, but the pinned source is authoritative.
4. Package the Android shared library with the expected `cdylib` entry point and verify the generated manifest and Activity superclass.
5. Keep any Kotlin/Java code to the thin GameActivity integration layer and SAF/lifecycle bridge required by the product brief.

References:
- [eframe 0.37.0 feature catalogue](https://docs.rs/crate/eframe/0.37.0/features)
- [winit 0.30.13 Android platform documentation](https://docs.rs/winit/0.30.13/winit/platform/android/)
- [cargo-ndk usage and supported targets](https://docs.rs/crate/cargo-ndk/latest)

### 8.4 Reproducible G1 runner recipe

On a Linux or macOS runner with Android Studio/NDK installed, record the exact installed NDK revision and use the repository's pinned Rust 1.95 toolchain:

```sh
rustup toolchain install 1.95.0 --profile minimal
rustup target add --toolchain 1.95.0 aarch64-linux-android
cargo +1.95.0 install cargo-ndk --locked

# Set this to the exact installed NDK directory; do not use a guessed version.
export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/<installed-ndk-version>"

rustc +1.95.0 --version --verbose
cargo +1.95.0 --version
cargo ndk --version
test -d "$ANDROID_NDK_HOME"

cargo +1.95.0 metadata --locked --format-version 1 > /tmp/photocraft-cargo-metadata.json
cargo +1.95.0 tree --locked --target aarch64-linux-android -e features \
  -p photocraft-raster -p photocraft-color -p photocraft-geom \
  -p photocraft-doc -p photocraft-compose -p photocraft-psd \
  -p photocraft-codecs -p photocraft-paint -p photocraft-text

cargo +1.95.0 ndk -t arm64-v8a check --locked \
  -p photocraft-raster -p photocraft-color -p photocraft-geom \
  -p photocraft-doc -p photocraft-compose -p photocraft-psd \
  -p photocraft-codecs -p photocraft-paint -p photocraft-text
```

Run optional codec-feature configurations separately when they are intended for the shipped Android product, and capture the exact command and resolved feature graph. Do not change `Cargo.toml` to hide a failing dependency. Record the first error, trace its originating crate/build script, and stop G1 if a required feature is unsupported.

The exact Cargo subcommand invocation above must be confirmed against the installed cargo-ndk version by capturing `cargo ndk --help`; do not assume an old cargo-ndk syntax.

### 8.5 Current G1 status remains blocked

This static review found no obvious platform-specific desktop dependency in the nine direct crate manifests, but it cannot establish target compatibility. The local execution environment still has no `rustc`, `cargo`, `cargo-ndk`, NDK, or GitHub network access. The fork has no Android build workflow and returned no Actions runs for the merged audit commit.

**G1 remains BLOCKED — not passed, not failed.** No source implementation work should proceed under the gate sequence until the above command has been run on a provisioned runner and all required packages compile successfully.
