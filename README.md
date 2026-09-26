![Adeline](assets/adeline-banner.png)

# Adeline

Agentic Development Environment offering many different modes of operations.


## Platform setup

The same source and pinned GPUI 0.2.2 dependency are used on Windows, macOS, and Linux. Application code has no platform-specific filesystem paths or shell integrations. Fonts, Windows console visibility, and window decoration differ by platform: Windows uses the custom title bar, while macOS and Linux retain their native decorations.

### Windows

Install Rust using the MSVC toolchain and Visual Studio Build Tools with **Desktop development with C++**, including a Windows SDK. A DirectX-capable graphics driver is required. This is the platform built and visually tested during implementation.

### macOS

Install Rust and the full Xcode application with its command-line tools. GPUI builds Metal shaders with Xcode:

```sh
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
cargo run --release --locked
```

### Linux

Install Rust and the native build dependencies. On Ubuntu / Debian, `scripts/linux` installs them (CI uses the same script):

```sh
./scripts/linux
cargo run --release --locked
```

GPUI's default features include X11 and Wayland. Run inside a graphical session with a working Vulkan driver. Consult [GPUI's documentation](https://docs.rs/gpui/0.2.2/gpui/) and [Zed's Linux build guide](https://github.com/zed-industries/zed/blob/main/docs/src/development/linux.md) for platform prerequisites.

## Validation

The checks follow [Zed's setup](https://github.com/zed-industries/zed). `rust-toolchain.toml` pins the Rust version, so rustup installs the right toolchain the first time you run `cargo`.

```sh
cargo fmt --all -- --check
./scripts/clippy
./scripts/check-todos
cargo nextest run --locked --all-features
cargo build --release --locked
```

On Windows, use `scripts\clippy.ps1`. Running `cargo test --locked` works too if nextest isn't installed.

`scripts/clippy` is the type checker and linter, the Rust counterpart of basedpyright. The lint levels are in `[lints]` in `Cargo.toml`: the default clippy groups plus `pedantic`, and Zed's denies (`dbg!`, `todo!`, redundant clones, disallowed methods). To silence a lint, use `#[expect(lint, reason = "...")]` rather than `#[allow]`. `expect` fails once the lint stops firing, like basedpyright's unnecessary-ignore check. `clippy.toml` bans calls that block the UI thread. Warnings are errors in both the script and CI.

If they are installed, `scripts/clippy` also runs these tools:

- [`cargo-shear`](https://github.com/Boshen/cargo-shear) finds unused dependencies.
- [`typos`](https://github.com/crate-ci/typos) spell-checks the code, using `.config/typos.toml`.

To install them:

```sh
cargo install --locked cargo-shear typos-cli cargo-nextest
```

The tests verify the bundled projects and live-demo state, search/status/completed filters, linked demo content, and notification totals as conversations change.

`.github/workflows/ci.yml` follows the layout of Zed's `run_tests` workflow. First comes a style job (rustfmt, TODO check, typos). Then clippy and nextest run on Windows, macOS, and Linux, alongside a dependency job (cargo-shear, lockfile, dependency review). Release builds run last, and a `tests_pass` job gives branch protection a single check to require. CI copies `.cargo/ci-config.toml` so that compiler warnings fail the build. The workflow is commented out for now, so it does not run. macOS and Linux have not been run locally on this Windows host; the workflow must run on those hosts to confirm their builds.

## Source map

| File | Responsibility |
| --- | --- |
| `src/main.rs` | App shell, design values, shared controls, startup |
| `src/settings.rs` | Settings window, search, shared mode controls, embedded license notices |
| `src/views.rs` | Workspace views, popups, dialogs |
| `src/chat.rs` | Chat entities, cache invalidation, virtual list state |
| `src/chat_render.rs` | Chat rows, message rows and composer presentation |
| `src/content_views.rs` | Independent file/service regions and virtual document/log views |
| `src/document_render.rs` | Prepared document rendering and background parse coordination |
| `src/prepared.rs` | Reusable document blocks, search snapshots and request generations |
| `src/ui_metrics.rs` | Optional native render counters and large test fixtures |
| `src/interaction.rs` | Local interactions and mocked actions |
| `src/data.rs` | Demo data model, scene state, filtering |
| `src/input.rs` | GPUI text input, selection, clipboard, IME |
| `assets/` | Bundled workspace data, artwork, and SVGs |
| `build.rs` | Embed assets for portable executable builds |
