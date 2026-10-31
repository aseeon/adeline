Status: Applied, except finding 1.

# Ponytail review: dead code after the Islands redesign

An audit of the whole repo, on `main` at cd03b2b, for code the redesign left dead or unneeded. Findings are ranked by size of the cut. Findings 2 to 8 were applied, and the README images moved to `docs/images/`. Finding 1 was declined: the five modes stay, and the design's Board, Terminal, Browser and Usage joined the rail, hidden until each is built.

1. `delete:` Remove the Workflows, Services, Groupchats, Issues and Whiteboard modes. The Islands design (`docs/design/DESIGN.md`) lists Chats, Docs, Board, Terminal, Browser and Usage instead. All five render an empty `div()` (`crates/adeline_ui/src/lib.rs:780`), and Groupchats is on by default, so new users get a blank mode in the rail. Drop the five `Section` variants, their `Features` fields with `enabled`/`toggle` arms and the legacy test, the `RAIL` rows (`crates/adeline_ui/src/project_bar.rs:35`), the `MODES` rows (`crates/adeline_ui/src/settings.rs:97`), and `workflow.svg`, `service.svg`, `whiteboard.svg`. `Features` already skips unknown flags in old config files, and `Section` is never saved to disk.
2. `delete:` Remove 17 icons in `assets/` that no code names: `adeline.svg`, `adeline.extreme.close.up.svg`, `arrow-elbow-down-left.svg`, `chat-fill.svg`, `chat-illustration.svg`, `command.svg`, `control.svg`, `house.svg`, `logo.svg`, `menu.svg`, `minus.svg`, `pin.svg`, `play.svg`, `project-square.svg`, `project-triangle.svg`, `text-align-left.svg`, `working.svg`. The assets `build.rs` embeds every file in `assets/` into the binary, so each one ships. Also drop their rows in `assets/PHOSPHOR.md`.
3. `delete:` Remove `scripts/check-ui-layout.ps1`. Nothing calls it, and the `adeline ui` control commands and the `Ui` tests replace it.
4. `native:` Two README images are embedded in the binary: `assets/adeline-banner.png` and `assets/adeline-screenshot.png`, 516 KB together. Move them to `docs/`, or have the assets `build.rs` skip them.
5. `yagni:` `BarColors` in `crates/adeline_ui/src/theme.rs:534` is a struct with one field and one caller. Inline `theme.border.mix_oklab(theme.title_bar, 0.5)` at `crates/adeline_ui/src/settings.rs:1537`.
6. `shrink:` The `palette!` macro in `crates/adeline_ui/src/theme.rs:28` generates 32 color accessors, and 23 are never called. An `#[allow(dead_code)]` hides that. Generate only `ROLES` and read colors through one `color("primary")` function. That removes the allow, so the compiler flags dead code again.
7. `delete:` `docs/scope-headless-ui-tests.md` (now in `docs/archive/`) still said `Status: Draft`, but the work has shipped. It also points at `project_bar/tabs.rs`, which the redesign deleted. Archive it as AGENTS.md describes.
8. `delete:` The worktree `.claude/worktrees/adeline-window-redesign-293440` is still checked out at 97c3bd8, which is already in `main`. Run `git worktree remove` on it and delete its branch. It's gitignored, so this is disk space only.

The actions, keybindings, config fields and UI state were checked too: each one still has a reader. The compiler already catches unused private functions, because every UI module is private.

net: about -130 lines and 20 asset files, -0 deps possible.
