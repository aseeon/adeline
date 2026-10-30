---
name: mockup
description: Build HTML mockup proposals of Adeline UI, iterate on them in rounds, then implement the winner. Use when asked for mockups, design proposals or variations of a screen, or to implement a chosen mockup.
---

Before the first round, load the `gpui-kit-design-guides` and `frontend-design` skills.

## Rounds

- Start from a real screenshot of the current app: the user's, or one you take with `adeline ui shot` from `adeline --demo --control`. Recreate the screen around the part being redesigned, so each proposal looks like the real app.
- Use only colors from the active theme, `bundled-themes/claude-plus.yml` by default, and Phosphor icons. When the user names an icon from a screenshot, use exactly that one.
- Keep one HTML file per topic, `artifacts/<topic>-mockups/index.html` (the folder is git-ignored), and add every round to it.
- A round has 4 proposals unless the user gives another number. Labels keep counting across rounds (A to D, then E to H). Variations of a pick get its letter plus a number (E1 to E4). When asked, include an unchanged copy of the pick so the user can compare.
- Under each proposal, write one line on how it differs from the pick.
- Keep earlier proposals until the user says to remove them.
- In a proposal the user asks to edit, change only what they named.
- Fill the mockup with enough demo data to show the hard cases: long titles, many items, every group and state, a narrow window. Make it interactive where the behavior is the point (hover, scroll, stacking).
- When a proposal needs something gpui-kit lacks (a custom animation, a blur, an element type), say so next to it.
- Open the file in the built-in browser pane after each round.

## Implementing the winner

When the user picks a proposal and says to implement it, build it in Adeline with the `gpui-kit` skill. Before writing code, list every element of the chosen proposal: each control, its style, its icon and its states. Take a screenshot of the built screen in `--demo` mode and compare it with the mockup. The work is done when every listed element is in the app and matches the mockup, in demo mode and with a real engine. Then follow the Completion steps in `AGENTS.md`.
