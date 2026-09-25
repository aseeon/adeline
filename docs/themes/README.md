# Themes and settings

## Bundled themes

Adeline embeds [bundled-themes](../../bundled-themes) and installs missing theme files on startup. See [theme sources](SOURCES.md) for attribution.

## Settings format

### Workspace color roles

| Surface / state | Background | Foreground |
| --- | --- | --- |
| Title bar, project bar, bottom control bar, main pane | Background | Foreground |
| App title | Background | Muted Foreground |
| Selected project | Sidebar Background | Sidebar Foreground |
| Unselected project | Background | Foreground |
| Unselected project or mode on hover | Popover | Popover Foreground |
| Composer text area | Secondary | Secondary Foreground |
| Selected mode | Card | Card Foreground |
| Mode bar, unselected mode, side panels, composer controls | Sidebar Background | Sidebar Foreground |
| Unselected chat filter | Sidebar Background | Sidebar Foreground |
| Selected or hovered chat filter | Card | Card Foreground |
| Unselected conversation | Sidebar Background | Sidebar Foreground |
| Hovered unselected conversation | Muted | Muted Foreground |
| Selected conversation | Sidebar Primary | Sidebar Primary Foreground |
| Notification badge | Primary | Primary Foreground |

Project tabs reserve 1-pixel top, left, and right borders inside their sizing. They use Border on the selected tab and are transparent on other tabs, so selection never shifts the layout. A 1-pixel Border divider separates the project and mode bars, interrupted beneath the selected tab so it joins the mode bar.

The Attention filter label uses Primary while its count is positive, including on hover. The selected filter underline uses Primary. Selected tabs, modes, filters, and conversations do not change on hover; these controls have no focus borders or press effects. Structural dividers use Border. Search fields use Input backgrounds, Border outlines, and Ring for existing focus highlights. Services notifications sit over the Services icon.

Adeline creates `~/.config/adeline/settings.yml` and the `themes` folder on first start. On Windows, `~` is `%USERPROFILE%`, so the location is `C:\Users\aseeon\.config\adeline` for this account. The executable embeds the 15 YAML files in `bundled-themes`: 14 copied from the user configuration on September 25, 2026, and `rose.yml` added on September 26, 2026. Startup seeds each missing file verbatim, including the customized lightos palette; existing files are never overwritten. Builds use this repository snapshot, not the build machine's user configuration. Existing YAML files take precedence over defaults. The old AppData `theme.json` is no longer read or changed.

The directory contains:

- `settings.yml`: General / Features, Appearance, Keymap; Modes / Chats, Docs, Workflows, Services.
- `themes/lightos.yml`: lightos, the default light theme.

Each theme starts with `name` and `brightness`, followed by a `colors` mapping containing exactly 32 named hex colors. Brightness accepts `Very Dark`, `Dark`, `Light`, or `Very Light`. Quote hex values so YAML does not treat them as comments. For example:

```yaml
name: My theme
brightness: Very Dark
colors:
    background: '#0F1218'
    foreground: '#F1F2F4'
    # Include the other 30 roles from a bundled file.
```

Copy a bundled file to create a theme. **Settings > General > Appearance** scans `.yml` and `.yaml` files each time the theme dropdown opens, so new files appear without restarting. Options show the theme name and filename. Invalid files are excluded with an explanation. Selection saves the filename in `general.appearance.theme` and refreshes all windows. **Apply colors** writes to the selected theme file; **Reload saved colors** discards drafts and reads that file again. Selecting another theme also discards unapplied drafts. Brightness controls light/dark-dependent blends.

Settings toggles persist immediately, including panel visibility for each mode. `general.appearance.interface_font` accepts `System` or a font-family name and is also editable in Appearance. Keymap values are lists of GPUI keystrokes such as `ctrl-,`; edit them in `settings.yml` and restart. Keymap displays the configured shortcuts. Enter/Space control activation and license text are built-in reference information rather than settings.

Manual settings edits load on restart. Theme files load at startup and when selected. Missing settings fields take their defaults; unknown fields or invalid values report an error. Invalid or unreadable files are preserved. Startup falls back to safe defaults and explains errors in Appearance. Save errors are shown without reporting success. YAML saves use a sibling temporary file and rename; comments are not retained when saving through the app.

`src/config.rs` defines the settings schema and defaults. `src/theme.rs` defines the 32 roles, bundled light palette, validation, and theme discovery. Rendering reads one resolved palette on the GPUI UI thread; switching themes invalidates cached views across windows.
The light colors come from [LifeOS-V1 on tweakcn](https://tweakcn.com/themes/cmnxpdhkt000004l49lq9b07w), with attribution recorded in [SOURCES.md](SOURCES.md). The export's OKLCH colors were converted to sRGB for the native renderer.

| Role | Light |
| --- | --- |
| Background | `#FCFBF7` |
| Foreground | `#1C1C1A` |
| Card / popover | `#FAFEFF` |
| Card / popover foreground | `#1C1C1A` |
| Primary / ring | `#BB5E3A` |
| Primary foreground | `#FAFEFF` |
| Secondary | `#E9E7E0` |
| Secondary foreground | `#4A4A44` |
| Muted | `#F2F1EB` |
| Muted foreground | `#7A7A72` |
| Accent | `#ECB37E` |
| Accent foreground | `#FCFBF7` |
| Destructive | `#E15A44` |
| Destructive foreground | `#FFFFFF` |
| Border / input | `#E2E0D5` |
| Chart 1 | `#BB5E3A` |
| Chart 2 | `#ECB37E` |
| Chart 3 | `#A2B771` |
| Chart 4 | `#4B6584` |
| Chart 5 | `#B894DB` |
| Sidebar | `#F7F6F0` |
| Sidebar foreground | `#1C1C1A` |
| Sidebar primary / ring | `#BB5E3A` |
| Sidebar primary foreground | `#FAFEFF` |
| Sidebar accent | `#EDEBE4` |
| Sidebar accent foreground | `#CE8264` |
| Sidebar border | `#E2E0D5` |

All interface colors resolve through theme roles, including the cursor, project swatches, previews, completion indicators, modal overlay, and shadows. Category backgrounds, project tints, and document preview surrounds blend theme colors. Project selections store a palette slot rather than a fixed RGB value, so switching or customizing themes updates them immediately. Native fonts and geometry remain in use. Embedded artwork and document images retain their original pixels. Settings and theme selection persist in YAML files.

