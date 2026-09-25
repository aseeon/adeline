# Adeline

Agentic Development Environment offering many differnt modes of operations.


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

Install Rust and the native build dependencies. On Ubuntu / Debian:

```sh
sudo apt-get update
sudo apt-get install build-essential clang libclang-dev cmake pkg-config \
  libfontconfig1-dev libfreetype6-dev libxcb1-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan-dev \
  libssl-dev libdbus-1-dev libasound2-dev zlib1g-dev
cargo run --release --locked
```

GPUI's default features include X11 and Wayland. Run inside a graphical session with a working Vulkan driver. Consult [GPUI's documentation](https://docs.rs/gpui/0.2.2/gpui/) and [Zed's Linux build guide](https://github.com/zed-industries/zed/blob/main/docs/src/development/linux.md) for platform prerequisites.

## Validation

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

The tests verify the bundled projects and live-demo state, search/status/completed filters, linked demo content, and notification totals as conversations change. `.github/workflows/build.yml` builds and tests separately on Windows, macOS, and Linux. macOS and Linux have not been run locally on this Windows host; the workflow must run on those hosts to confirm their builds.

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
