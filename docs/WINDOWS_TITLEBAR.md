# Windows title bar

Adeline draws its Windows title bar in GPUI using the existing warm gray chrome, system UI font, and muted foreground. The caption shows the project name and Adeline; the three buttons have neutral hover states and a red Close state. Maximized windows show the Restore glyph. Inactive windows use the muted foreground.

## Adeline implementation

The pinned GPUI 0.2.2 version already implements this mechanism in `platform/windows/window.rs` and `platform/windows/events.rs`. No dependency update, Win32 shim, or simulated mouse-button action is needed.

`src/titlebar.rs` renders a 36-pixel bar. The window buttons and their hover backgrounds fill its full height up to the top edge. A four-pixel resize strip is reserved only above the draggable caption. Caption and control regions are separate. Buttons deliberately have no `on_click` handler, avoiding duplicate native actions and preserving caption hit testing. Caption controls use regular-weight Phosphor SVGs (minus, square, copy, and x), inheriting the active, inactive, and hover foreground colors.

`src/main.rs` enables the transparent titlebar only on Windows and redraws when window bounds or activation change. The title bar sits outside the content overlay container, keeping window controls usable while a picker or dialog is open. The native window title is still maintained for Alt+Tab and the taskbar.

Settings uses the same separation: its focus tracking and keyboard actions belong to the content row below the caption. Do not put `track_focus` on a shell containing the title bar. GPUI 0.2.2's focus mouse-down handler calls `prevent_default`, which consumes native caption mouse-down events and prevents Windows from starting a drag.

macOS and Linux retain their existing system decorations. Windows-only rendering is guarded at compile time; all application content continues to use the same cross-platform GPUI implementation.

The current upstream Zed implementation differs from GPUI 0.2.2 in some low-level details, especially edge-resize calculations. Adeline uses the published dependency's native implementation instead of copying the newer backend into the app.

## Performance

Use `cargo run --release --locked` or the prepared `dist/Adeline.exe` when comparing dragging with Zed. The development profile leaves GPUI unoptimized. Native movement still pumps painting on the UI thread, so slow layout and drawing can delay movement. A measured comparison reduced CPU drawing work after movement from about 32 ms to 4 ms by switching to release, without changing the title bar or its event handling.
