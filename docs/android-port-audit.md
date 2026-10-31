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
