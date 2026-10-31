# Adeline design

Adeline is a native desktop app for running coding agents across your projects. It is an ADE (Agentic Development Environment).

Users chat with Claude Code, Codex, OpenCode, oh-my-pi and any other ACP agent in one app. Adeline is built with Rust and GPUI for native performance.

## Redesign

The redesign uses the Islands style. `DESIGN.md` describes it, and `design.html` is the working reference. `prototype/index.html` keeps all three explored variants. Follow `DESIGN.md` when you design or change Adeline's UI.

## Current look

`current-main-view.png` shows the main view as it looks in the shipped app today, before the redesign.

The main view has these parts:

- Title bar with project tabs (for example "ade-project · Nexus"), a machines picker and a projects picker.
- Left sidebar with chats grouped by state and date: Needs input, Processing, Today, Last 3 days, Earlier.
- Center chat column with user and agent messages, file attachments, a thinking indicator and queued messages (Edit, Remove, Send now).
- Composer at the bottom with agent, model, effort and mode pickers.
- Right "Agent activity" panel with a per-turn step list and stats: active time, tool time, succeeded, failed, context use and reply speed.

The theme is dark and neutral, with orange used for unread and attention dots.
