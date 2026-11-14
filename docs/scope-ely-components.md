Status: Confirmed

# Ely components beside GPUI Kit

## Purpose and context

Changes mode (`docs/scope-changes-mode.md`) needs a diff viewer and a read-only code viewer. Ely's `ely-git` crate has `DiffViewer`, and `ely-editor` has `CodeEditor` with a read-only mode. Ely runs beside GPUI Kit in one window only when both build on the same `gpui-pre`, and Adeline's GPUI is older than Ely's. This scope moves Adeline to Ely's GPUI and starts Ely's components next to Kit's, with no change a user can see. Changes mode then uses them.

Current behavior:

- The workspace pins `gpui-kit = "=0.7.0"` (root `Cargo.toml`), which pins `gpui-pre =0.3.7`. No crate names `gpui` directly.
- The toolchain is Rust 1.98.1 (`rust-toolchain.toml`). Ely needs 1.98.
- Ely's guide (https://elygpui.com/docs/gpui-kit, read 2026-11-14) says Ely pins `gpui-pre =0.3.11`, the snapshot Kit 0.9.0 pins. Ely's `main` (commit `078bf26f2d03`, 2026-11-14) has since moved to `=0.3.12`, which Kit 0.9.1 pins. The crates.io release `ely-git` 0.1.1 pins `=0.3.10` and matches neither.
- Ely is published from `https://github.com/ELY-Open-Source/Ely-GPUI-Components` (MIT). The guide adds it as the git dependency `ely-gpui-component`. The repo also has the `ely-git`, `ely-editor` and `ely-core` crates.

## Requirements

- R1. Adeline builds on the `gpui-pre` version that the chosen Ely commit pins, with the GPUI Kit release that pins the same version. Today that is `gpui-pre =0.3.12`, `gpui-kit =0.9.1` and Ely commit `078bf26f2d03` (see D1).
- R2. Ely comes from its git repository at a fixed commit (`rev`), never a moving branch, so `--locked` builds stay reproducible. The UI takes only the Ely crates Changes mode needs (`ely-git`, `ely-editor` and what they depend on), or `ely-gpui-component` if the split crates can't be taken alone.
- R3. Only `adeline_ui` depends on Ely. The headless build (`--no-default-features`) compiles no Ely and no GPUI, as now.
- R4. Every API change from Kit 0.7.0 to the new Kit is fixed in Adeline's code. The app looks and behaves as before: same layout, themes, fonts, shortcuts, dialogs and accessibility labels. The existing UI tests, pixel tests and control commands (`adeline ui …`) pass without changes to what they assert. Changes to test helpers that only follow a renamed Kit API are allowed.
- R5. The app starts both libraries as Ely's guide prescribes:
  - one asset source, `Assets::before(gpui_kit::assets::Assets)`, so Ely's files load from Ely and all other paths from Kit, combined with Adeline's own assets (`adeline_assets`);
  - `gpui_kit::init(cx)` and `ely_gpui_component::init(cx)?` (or the init of each Ely crate used), in either order;
  - windows still opened through Kit (`gpui_kit::open_window`), so Kit's dialogs, sheets and notifications keep working;
  - each Ely view wrapped in Ely's `FocusScope`, with its handle focused.
- R6. If Ely's init fails because one of its files did not load, the app still opens. Ely's views are then unavailable, and the failure goes to the log.
- R7. Ely's theme follows Adeline's theme. Every theme change, including light and dark switches and the live reload of theme files, sets Ely's mode with `Theme::set_mode` next to Kit's `Theme::change`. Ely's colors are mapped from Adeline's twelve theme colors where Ely allows it, so an Ely view doesn't stand out from the Kit views around it.
- R8. Tab and Shift+Tab keep their current behavior in every Kit view, including indenting in Kit's code editor. Inside Ely views they move between Ely's focus stops in the same order as Kit's.
- R9. Ely's locale follows Kit's, if Adeline sets one.
- R10. Debug builds install Ely's inspector after Kit's (`ely_gpui_component::tooling::install_inspector`).
- R11. Settings › Licenses lists Ely and the third-party assets it bundles, as it does for Kit's.
- R12. The upgrade works in `--demo`, in control builds, and in the dist package on Windows, macOS and Linux.
- R13. `docs/architecture.md` says that the UI uses Ely beside Kit, where Ely is pinned, and how the two are started and themed.
- R14. The version in `[workspace.package]` stays the same, since nothing changes for the user. `PROTOCOL` stays the same.

## Boundaries

- No Ely view appears in the app yet. Changes mode adds the first ones.
- The engine, the core and the protocol don't change.
- Ely's components replace no Kit component that Adeline already uses.

## Failure and edge cases

- R15. If Kit and Ely ever pin different `gpui-pre` versions after this scope, Adeline stays on the last pair that match. A Kit upgrade waits until the pinned Ely commit matches it.

## Acceptance criteria

- AC1 (R1, R2). `Cargo.lock` holds one `gpui-pre`, at the version the pinned Ely commit names. The root `Cargo.toml` pins Kit exactly and names Ely by `rev`.
- AC2 (R3). `cargo tree -p adeline --no-default-features` shows no `gpui-pre` and no `ely-` crate.
- AC3 (R4, R12). All checks in `AGENTS.md` pass: fmt, `check-todos`, clippy (full and headless), nextest with all features and headless. `cargo build --release --locked --features control` succeeds.
- AC4 (R4). `adeline --demo --control --hidden` opens, and `adeline ui shot` of Chats, Docs and Settings matches the previous release by eye.
- AC5 (R5). A test opens an Ely view inside the main window in demo mode, and it draws with Ely's files loaded.
- AC6 (R6). With Ely's init forced to fail in a test, the window still opens and the log has the error.
- AC7 (R7). Switching between a light and a dark theme changes the colors of a test Ely view and of the Kit views around it.
- AC8 (R8). Tab still indents in Docs mode's Editor, and Tab moves focus through an Ely view's stops.
- AC9 (R10). A debug build opens Ely's inspector next to Kit's.
- AC10 (R11). Settings › Licenses lists Ely.
- AC11 (R13, R14). `docs/architecture.md` has the Ely notes. The workspace version and `PROTOCOL` are unchanged.
- AC12 (R9). With a Kit locale set, the Ely test view reads the same locale.
- AC13 (R15). `docs/architecture.md` states that Kit upgrades wait for a matching Ely commit.
- AC14 (R12). The dist package builds on CI for every platform and opens on Windows.

## Decisions and rationale

- D1. Target `gpui-pre =0.3.12` with Kit 0.9.1 and Ely `main` at `078bf26f2d03`. The request named 0.3.11, the version on Ely's guide page, but Ely's own manifest has moved on to 0.3.12. Staying on 0.3.11 would mean Kit `=0.9.0` and an older Ely commit, missing the latest Ely fixes. Switch to that pair if 0.3.12 turns out broken.
- D2. A git dependency at a fixed commit, since the crates.io Ely releases trail `main` and pin an older GPUI.

## Open questions

None.
