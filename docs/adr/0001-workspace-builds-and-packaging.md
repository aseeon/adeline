# Workspace, builds and packaging

Adeline is one Cargo workspace that builds one binary in two shapes: the desktop app and a headless build for remote machines. These decisions keep local rebuilds fast and keep GPUI out of the headless build.

## Workspace

- `default-members` is `crates/adeline`, so a plain `cargo build` builds the app. Tests and clippy pass `--workspace` to cover every crate.
- `adeline_core` has no GPUI. The engine and the headless build depend on it, so a GPUI type there would pull desktop libraries into them.
- The `gui` feature of `adeline_engine` changes no behavior. The engine only reports it in its status, so clients can tell a headless engine from a full one.
- `adeline_engine` uses the ACP SDK without its `process` feature. Adeline starts agent processes itself, inside job objects and process groups (see ADR 0008).

## Features

- `control` (in `crates/adeline` and `crates/adeline_ui`) is for local release builds only. The dist package never has it, since it lets another process drive the UI.
- `test-support` in `crates/adeline` turns on `control` and `gpui-kit` for `tests/ui.rs`. A dev-dependency on the UI crate would build GPUI into the headless tests too.
- `gpui-kit` is an optional dependency of `crates/adeline` on purpose, since only `tests/ui.rs` uses it. `cargo-shear` is told to ignore it.

## Profiles and linking

- Local release builds skip LTO and build incrementally. A one-line edit rebuilds in about 14 s instead of 78 s. Thin LTO alone took about 60 s.
- The `dist` profile keeps thin LTO. `scripts/package.ps1`, `scripts/macos-app` and the CI artifacts use it.
- Windows links with `rust-lld.exe` (`.cargo/config.toml`). It links the app in about 2 s, where MSVC's `link.exe` took 3.5 s, the slowest step of a rebuild on Windows. The macOS linker is already fast, and x86_64 Linux uses rust-lld by default.

## Headless Linux builds

- The headless Linux build is static musl, so it runs in Alpine and on hosts with no desktop libraries.
- mimalloc is C, so a musl build needs musl's C compiler (`musl-tools`).

## macOS package

- macOS ships an app bundle, `target/dist/Adeline.app`. Finder runs a bare executable in Terminal.
- `scripts/macos-app` takes the icon from the newest `target/dist/build/adeline-*/out` folder, because `build.rs` writes it there and older build folders hold old copies.
- The script signs the bundle ad hoc. Copying the binary into a bundle breaks its ad hoc signature, and Apple silicon runs only signed code. A Developer ID signature would also get past Gatekeeper.
