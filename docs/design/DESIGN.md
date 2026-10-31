# Adeline design: Islands

Islands is the chosen direction for Adeline's main view. The window is one dark surface. Two floating islands sit on it: navigation on the left, agent details on the right. The chat runs between them with no box around it, and the composer floats at the bottom like a third island.

The reference build is [design.html](design.html), which holds only Islands. [prototype/index.html](prototype/index.html) still has all three explored variants. Every value below comes from design.html. When this document and design.html disagree, design.html wins, so fix this document.

Screenshots at 1440×900, 2× scale:

- [Both islands open](screenshots/islands-1-both-islands-open.png)
- [Both islands closed](screenshots/islands-2-both-islands-closed.png)
- [Project picker open](screenshots/islands-3-project-picker.png)
- [Model menu open from the composer](screenshots/islands-4-model-picker.png)

## Layout

All numbers are in logical pixels at 1× scale.

| Part | Size and position |
| --- | --- |
| Window | Radius 12. A 1px ring of white at 14%, plus a shadow `0 24px 60px`. |
| Window padding | 8 on all sides. The gap between columns is 8. |
| Islands, top | 34 margin, so 42 from the window's top edge. |
| Islands, bottom | 13 margin, so 21 from the window's bottom edge. That's half the top gap. |
| Island shape | Radius 14. A 1px `--ring`, plus a shadow `0 10px 30px` at 30%. |
| Left island | A 56 rail, then a 286 chat list. |
| Right island | A 330 pane, then a 56 rail. The rail stays visible when the pane is closed. In other modes the pane holds the chat and is 440. |
| Chat header | 26 tall, starting at the 8 padding. Its text centers at 21, the middle of the 42 gap. |
| Window controls | Three 46×40 buttons in the top right corner. They sit in the gap above the right island. |
| Chat column | Max width 760, with 28 side padding. Agent prose stops at 68ch. |
| Composer | Its bottom sits 21 from the window's bottom edge, level with the islands. |

The header text is "Mode / project / item" in every mode. The mode and project are in `--fg2`, each `/` is in `--fg3`, and the item is in `--fg` at weight 500. It centers over the main area. Nothing else goes in the header. Buttons for a chat's side details belong on the right rail.

| Mode | Header |
| --- | --- |
| Chats | Chats / project / chat title |
| Docs | Docs / project / file name |
| Board | Board / project / board name |
| Terminal | Terminal / project / working directory, relative to the project (`.` at the root, `./src/chat` below it) |
| Browser | Browser / project / domain of the current tab |
| Usage | Usage / filter (no project, because the filter picks the projects) |
| Settings | Settings |

The gap above the islands is 2× the gap below them. That leaves room for the header and window controls without a title bar. Keep the 42/21 ratio if you change either number.

## Surfaces

Brightness always goes up in this order: islands, then chat (the window background), then composer and cards. Each step tells the eye what to read first. Never put a lighter surface behind a darker one.

| Token | Role | Dark grey (default) | A color, hue H |
| --- | --- | --- | --- |
| `--isl-l` | Both islands | `oklch(0.185 0 0)` | `oklch(0.335 0.08 H)` |
| `--bg` | Window and chat | `oklch(0.215 0 0)` | `oklch(0.4 0.085 H)` |
| `--card` | Ask cards, queue | `oklch(0.24 0 0)` | `oklch(0.46 0.085 H)` |
| `--raise` | Composer | `oklch(0.25 0 0)` | `oklch(0.46 0.085 H)` |
| `--pal-bg` | Project palette, tooltips | `oklch(0.25 0 0)` | `oklch(0.42 0.085 H)` |
| `--bubble` | User messages | `oklch(0.28 0 0)` | `oklch(0.49 0.09 H)` |

The colors are Dark grey (the default), Periwinkle 265, Harbor 215, Moss 160 and Clay 20. A color only changes hue and chroma. The order of the steps stays the same.

These overlays sit on top of any surface. They're white at a fixed opacity, so they work on every color:

| Token | Value | Use |
| --- | --- | --- |
| `--line` | white 9% | Dividers, like the line between a rail and its island content |
| `--hover` | white 7% | Hover background |
| `--chip` | white 8% | Inline code, file chips, the project switcher card |
| `--sel` | white 12% | Selected row or tab |
| `--ring` | white 10% | 1px ring around islands, cards and the composer |
| `--ring-strong` | white 22% | Composer ring while it has focus |

Use a 1px `box-shadow` ring for edges, not borders. Rings don't change layout and look the same on every color. The only true divider is `--line`, between a rail and its island content.

### Glass

Glass is an optional surface. Islands become translucent over the desktop with `blur(36px) saturate(1.6)`. In Dark grey, `--bg` is `oklch(0.2 0 0 / 0.5)` and `--isl-l` is `oklch(0.1 0 0 / 0.38)`. Glass works best on macOS, mostly works on Windows and is clumsy on Linux. Linux falls back to the solid surface.

### Light theme

Settings › Appearance › Theme switches between Dark and Light. Light uses the same tokens with new values, set under `[data-theme="light"]` in design.html. The overlay tokens (`--line`, `--hover`, `--chip`, `--sel`, `--ring`, `--ring-strong`) read their color from `--ink`. It's white in Dark and black in Light, at the same opacities.

The brightness order holds in Light too: islands, then chat, then composer and cards. In Light grey, `--isl-l` is `oklch(0.935 0 0)`, `--bg` is `oklch(0.965 0 0)` and `--raise` is white. Text is `oklch(0.2 0 0)`, `oklch(0.42 0 0)` and `oklch(0.56 0 0)`. Status colors get darker so they read on light surfaces. The menu becomes light grey. Send uses `--sendfg` for its icon, because Light puts a dark button behind it.

## Text

| Token | Dark grey | A color | Use |
| --- | --- | --- | --- |
| `--fg` | `oklch(0.94 0 0)` | `oklch(0.97 0.01 H)` | Titles, unread rows, message text |
| `--fg2` | `oklch(0.74 0 0)` | `oklch(0.84 0.03 H)` | Read rows, labels, pickers |
| `--fg3` | `oklch(0.56 0 0)` | `oklch(0.7 0.04 H)` | Times, counts, placeholders, meta |

The UI font is Segoe UI Variable Text, then the system UI font. Code uses JetBrains Mono, then Cascadia Code. All numbers use tabular figures so counts and timers don't shift.

Every size comes from the scale below, set as tokens on `:root` in design.html. Never write a raw size. If no step fits, the design needs a different role, not a new number. UI steps grow by about 1.08, so dense text stays close in size and weight does the work. Heading steps grow by about 1.2, so headings stand apart from the text under them.

| Token | Size | Weight and leading | Use |
| --- | --- | --- | --- |
| `--t-micro` | 11 | 600, uppercase, 0.08em tracking | Group labels in the chat list and the Usage list |
| `--t-micro` | 11 | 400 | Keyboard hints, badge counts |
| `--t-caption` | 12 | 400, or 600 for section labels | Times, counts, meta lines, tooltips. Section labels in cards and board columns at 600 in `--fg2`. |
| `--t-ui` | 13 | 400 | Pickers, tabs, menus, file trees, plan and step lists, terminal |
| `--t-body` | 14 | 400, or 600 for titles | Chat rows, header, buttons. Island titles at 600 on both islands. |
| `--t-prose` | 15 | 400, leading 1.65 | Agent messages, ask cards, documents, the composer. User bubbles use leading 1.55. |
| `--t-input` | 16 | 400 | Palette search |
| `--t-title` | 17 | 600, leading 1.3 | Page and section headings, like Settings and document sections |
| `--t-display` | 20 | 600, leading 1.25, −0.01em | Empty states, big numbers in Usage |
| `--t-doc` | 24 | 600, leading 1.2, −0.012em | Document title |

- **Weights.** Use only 400, 500 and 600. `font-synthesis: none` stops the browser from faking a weight that wasn't loaded.
- **Mono.** Code runs one step below the text around it: 13 inside 15 prose, 12 inside 13 lists.
- **Monogram initials.** These are glyphs, not text. They're 9 in boxes up to 22, and `--t-micro` in boxes of 28 and up.
- **Headings.** They use `text-wrap: balance`. Prose blocks use `text-wrap: pretty` and stop at 68ch.
- **Floor.** Never set text below 11. Monogram initials are the only exception.
- **Hover and selection.** Never change font weight on hover or selection, because the text reflows. The one weight change is unread rows going to 500, which marks state and not interaction.

## Status colors

| Token | Value | Only for |
| --- | --- | --- |
| `--accent` | `oklch(0.78 0.15 55)` (orange) | Attention: Needs input dots, counts and labels, and the dot on the Chats mode |
| `--ok` | `oklch(0.82 0.14 150)` | Done plan steps, `+` line counts, a project with a running agent |
| `--err` | `oklch(0.75 0.15 20)` | Failed steps, `−` line counts, non-zero exit codes |
| `--ctx` | `oklch(0.72 0.13 250)` in Dark grey, `--sendbg` in a color | The context ring's fill |

Orange means "a person needs to act". Never use it for decoration, or users stop trusting it. Agent glyphs keep their brand colors in every theme. Claude Code is `#d97757`.

## Rails

The left and right rails must always look and behave the same. When you change one rail's size, spacing, colors, states, tooltips or motion, make the same change to the other rail at once. Style them with shared `.mrail` rules, and wire their behavior through shared code like `slideHighlight`. Only the buttons and their order differ.

Buttons are 38×38 with radius 10 and an 18 icon in `--fg3`. They sit in a column with a gap of 4 and 14 padding at the top and bottom. The active button gets `--ink` at 11% and `--fg`. Pressing scales it to 0.96. Clicking a button slides the highlight to it, as described in Motion.

The left rail holds, from the top: Chats, Docs, Board, Terminal, Browser, Usage, then Settings. Ctrl+1 to Ctrl+6 switch modes, and Ctrl+, opens Settings. The chat list toggle (Ctrl+B) is pinned to the bottom. It's the only pinned button.

The right rail holds Activity, Changes and Plan, with the pane toggle pinned to the bottom. Clicking a tab icon opens the pane on that tab. While the pane is open, the current tab's icon is active and the pane header shows its name (46 tall, 14/600).

### Chat in other modes

In every rail mode except Chats (Docs, Board, Terminal, Browser and Usage), the right island holds the last active chat with its own composer. You can keep talking to the agent while you work in another mode. Settings has no right island.

- The rail holds one Chat tab, with the pane toggle pinned to the bottom.
- The pane is 440 wide. The header shows the chat title, and the conversation and composer follow the Chats rules.
- The column padding drops to 14.
- The composer's agent picker shows only the agent glyph, so the bar fits. Its tooltip and label still name the agent.

```
Adding a button to a rail?
 ├── It switches what the main area shows → left rail, after the modes, before Settings
 ├── It switches what the right pane shows → right rail, after Plan
 ├── It shows or hides its own island → bottom of that island's rail
 └── Anything else → it doesn't belong on a rail
```

## Modes

Each mode fills the left island's list with its own content and the main area with its own view. The right island keeps the chat, as described in Chat in other modes. The header follows the rule in Layout.

| Mode | Left island | Main area |
| --- | --- | --- |
| Docs | File tree of the project's docs | Document editor: a toolbar (Heading, Bold, Italic, List, Saved) above the page. Text follows the agent prose rules: 15 at line height 1.65, at most 68ch. |
| Board | Boards, with a count of open tickets | Filter chips for label and agent, then three columns: To do, In progress, Done |
| Terminal | File tree of the project. Clicking a file adds its path to the prompt. | One shell in a card, mono 13 at line height 1.6 |
| Browser | Open tabs, each with a close button on hover | Back, Forward, Reload, the address, then the page |
| Usage | All machines, All projects (this computer), then projects grouped by machine | Range switch (7 or 30 days), four totals, tokens per day by agent, then By agent and By project (or By model for one project) |

There's no Files mode. The file tree lives in the left island of Docs and Terminal.

Rules for the parts modes share:

- **Cards.** The editor, terminal and browser sit in one card each: radius 14, `--card`, a `--ring`, with 8 padding above and 12 at the sides. Its bottom lines up with the islands. A toolbar on a card is 44 tall with a `--line` under it.
- **File tree.** Rows are 30 tall with radius 7 and 13 text. Each level indents 14. Folders have a chevron and open or close on click. The open file uses `--sel`.
- **Tickets.** A ticket is a card with radius 10: its ID in `--fg3`, the agent glyph, the title at 14, then a label chip. A ticket that needs input shows the orange dot and "Needs input". That's the only orange on the board.
- **Filter chips.** 28 tall, fully rounded, with a `--ring`. A chosen chip gets `--sel`.
- **Charts.** Bars use the agent colors, so they read the same in the legend, the bars and the glyphs. Other bars use `--fg2`. Tracks use `--fg3` at 25%.
- **Web pages.** A page in Browser keeps its own colors in every theme, because it's someone else's site. This is the one place raw colors are allowed besides agent glyphs.

## Chat list

- **Project switcher.** A card at the top of the chat list. It holds a 30px project monogram, the name at 14/600, the machine at 12 in `--fg3`, and a "Ctrl P" hint. Under it, other open projects show as 32px rows. A row ends in an orange count for chats that need input, or a green dot when an agent is running. A close button appears on hover or focus.
- **Rows.** 36 tall, radius 8, with three slots: agent glyph (16), title (14, truncated) and status. The status is a time ("2h", "Thu") in `--fg3`, an orange dot for needs input, or a spinner for processing. Never add a fourth slot. Put extra detail in the chat itself.
- **Groups.** In this order: Needs input (label in orange), Processing, Today, Last 3 days, Earlier. Each has a count on the right.

## Composer

The composer has radius 14, padding 6, `--raise` as its background and a `--ring`. While it has focus, the ring becomes `--ring-strong`. The textarea is 15px and grows up to 40% of the window height.

The bottom bar holds, from left to right: attach (+), agent, model, effort, a spacer, mode, the context ring and send. Pickers are 28 tall with a chevron. Send is 30×30 with radius 9, on `--sendbg`. It's disabled at 25% opacity until there's text.

The context ring is an 18px circle with a 2.5 stroke. The track is `--fg3` at 35%, and the arc fills to the share of context used. Hovering or focusing it shows "Context 54% · 108k of 200k tokens" after 200ms. Nothing goes below the composer. Context, branch and machine details live in the ring's tooltip, the header and the project switcher.

## Overlays

```
Choosing an overlay?
 ├── The user picks one value from a short list (agent, model, effort, mode) → menu
 ├── The user searches or switches projects → project palette
 └── The user needs a value explained → tooltip (no buttons inside)
```

- **Menu.** At least 220 wide, radius 10, padding 4, on neutral `oklch(0.24 0 0)` in every color. Items are 32 tall with 6 radius. The header is 12 in grey, and the chosen item has a blue tick. It opens upward from its picker in the composer. It scales in from 0.97 over 150ms.
- **Project palette.** Ctrl+P or the switcher card opens it. It's 580 wide and 88 from the window's top, with radius 14 and `--pal-bg`. A scrim dims the window behind it. The search input is 54 tall with 16px text. Rows are 48 tall with a 28 monogram. Open projects come first, then the other projects grouped by machine ("Nexus · This computer"). Arrow keys move, Enter switches and Escape closes.
- **Tooltip.** Radius 8, `--pal-bg`, 12px text. It appears after a 200ms delay.

## Motion

- Hover changes background and color over 150ms ease-out. Never animate layout, except when an island opens or closes.
- An island opens over 200ms and closes over 160ms, with `cubic-bezier(0.23, 1, 0.32, 1)`. Its width and opacity animate, so the chat column moves with it. The content keeps its full width and slides under the rail without rewrapping. Toggling again mid-way starts a fresh ease-out from the current width. With reduced motion, islands open and close instantly.
- Pressing a button scales it to 0.96, everywhere.
- Switching between Chats and any other mode moves the composer between the center and the right island, so you see where the chat went. It travels with `translate` over 220ms with `cubic-bezier(0.65, 0, 0.35, 1)`, and only its center moves, so it never scales. The conversation fades in over 150ms with a 12px drift in the same direction. It also runs on Ctrl+1 to Ctrl+6, because it tells keyboard users where the chat went too. The composer keeps focus while it moves. Switching between two other modes moves nothing. If the right island is closed, its Chat tab brightens once over 600ms instead. With reduced motion, nothing moves.
- Clicking a button on either rail slides the active highlight from the old button to the new one over 150ms, with `cubic-bezier(0.23, 1, 0.32, 1)`. Only the highlight moves. The icon, the main area and the pane content switch at once. Keyboard switches (Ctrl+1 to Ctrl+7, Ctrl+, and Enter on a focused button) don't slide. A tab that opens the closed right pane doesn't slide either, because there's no old highlight. With reduced motion, nothing slides.
- Menus take 150ms and the palette takes 180ms, both with `cubic-bezier(0.23, 1, 0.32, 1)`. Exits are instant.
- Thinking text shimmers, and spinners turn every 0.9s. With reduced motion, the shimmer stops and spinners slow to 3s.
- Scrollbars are thin and invisible until the pointer is over the area that scrolls. On touch screens they always show.

## Details

- **Focus ring.** It's 2px of `--focus`, which is `--ink` at 60%: white in Dark, black in Light. It's never tinted, because a colored ring clashes with every color option. Text fields show focus through their container's `--ring-strong` instead.
- **Stacking.** Use only the z-index tokens: `--z-float` 4 for the composer as it moves, `--z-chrome` 5 for the window controls, `--z-scrim` 10, `--z-palette` 11, then `--z-menu` 20 so menus open above everything. Inside a component, use `isolation: isolate` instead of a number.
- **Hit areas.** Dense desktop controls stay 28 to 38. Anything smaller grows its target with a `::before` up to 28 without overlapping a neighbor, like the 20px remove button on file chips.
- **Optical padding.** A picker ends in a chevron, so its right padding is 2px less than its left: 8 then 6.
- **Selection.** Button labels and kbd hints can't be selected. Messages, documents, terminal output and pages can.
- **Copy.** Use typographic apostrophes and the single `…` character. Code and terminal output keep their own characters.
- **Empty states.** An empty area says what belongs there and offers the action that fills it. With no tabs, Browser offers to open the app preview.
- **Reloads.** Theme, surface, color, open islands and the current mode survive a reload, so the first paint is already right. If storage is blocked, the page starts from the defaults.

## Checklist

- [ ] Every color comes from a token in this file. No raw hex except agent glyphs, the close button's red and web pages in Browser.
- [ ] Brightness goes up from islands to chat to composer, in every color.
- [ ] The islands sit 42 from the top and 21 from the bottom. The composer's bottom lines up with the islands.
- [ ] Orange appears only where a person needs to act.
- [ ] Chat rows have exactly three slots.
- [ ] Nothing sits under the composer or in the header besides the title.
- [ ] Each rail pins exactly one button to its bottom: the toggle for its own island.
- [ ] Both rails look and behave the same. A change to one rail is made to the other.
