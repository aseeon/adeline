Status: Implemented in v0.1.0. This scope is history. Where it and the code differ, the code is right.

# GPUI Kit UI rebuild

## Purpose and context

Rebuild Adeline's Main UI, Settings, and Chats using GPUI Kit. Improve look and feel, keyboard behavior, maintainability, and future framework compatibility by reducing the need to build and style custom controls. Preserve how the retained product works behind the scenes.

This benefits people using Adeline and maintaining its UI. The requester decides on structural layout changes and any proposed loss of existing functionality.

### Current behavior and evidence

- `Cargo.toml`: Adeline depends on `gpui-kit = "=0.7.0"`. This scope does not choose a new dependency version.
- `src/config.rs`, `Settings`, `Appearance`, `Keymap`, `Features`, and `Modes`: settings cover appearance, shortcuts, feature visibility, and mode preferences. `Features::enabled` keeps Chats available independently of optional modes.
- `src/settings.rs`, `SettingsWindow::render`, `open_agent`, and `Adeline::mode_options`: Settings includes General, Modes, Licenses, and Agents; agent creation has its own window. Excluded modes currently have content-specific and panel settings.
- `src/views.rs`, `Adeline::chats` and `Adeline::activity_content`; `src/chat_render.rs`, `Adeline::chat_card` and `Adeline::chat_sidebar`: Chats has a conversation list, status filters, transcript, composer, conversation actions, and agent activity.
- `README.md`, "Agents" and "Projects and conversations": existing behavior includes persisted agent definitions, projects and transcripts, streaming, permission requests, cancellation, retries, session recovery, and graceful shutdown.
- `src/theme.rs`, `ThemeFile::validate`, `discover`, `init`, and `select`: existing themes contain 32 named colors, load from YAML, and refresh open windows when selected. Invalid startup themes fall back to Claude Plus with an error. Tests `bundled_themes_populate_fresh_config_and_preserve_existing_files`, `discovers_added_themes_and_reports_invalid_files`, and `missing_theme_falls_back_to_embedded_claude_plus` establish preservation and failure behavior.
- `src/config.rs`, `update` and `Adeline::load_settings`: saving rereads the settings file, while current startup applies excluded-mode preferences. Tests `legacy_features_use_new_defaults_and_explicit_choices_persist` and `yaml_defaults_and_custom_settings_round_trip` cover feature choices and saved settings. This rebuild deliberately changes excluded-mode behavior and new-file contents.

### Reference material

Use the requested `gpui-kit` and `gpui-kit-design-guides` skills, including the normative [Design Guides](https://gpui-kit.com/docs/design-guides.md) and applicable [Coding Guides](https://gpui-kit.com/docs/coding-guides.md).

[Ghostex](https://github.com/maddada/Ghostex) is a reference, not an authority or a feature specification. Its inspected [desktop manifest](https://github.com/maddada/Ghostex/blob/main/apps/desktop/Cargo.toml) uses a local `gpui-component` dependency and patched GPUI sources. Its [desktop entry point](https://github.com/maddada/Ghostex/blob/main/apps/desktop/src/main.rs), `main`, initializes the component library, applies application theming, and wraps the application view in `Root`; it also customizes root background and Linux border behavior. These facts do not establish that those choices are appropriate for Adeline or available unchanged in published Kit releases. References were inspected on 2026-09-27; upstream `main` may change.

## Requirements

### Included surfaces and retained behavior

**R1.** Rebuild the shared Main UI, Settings, and Chats with GPUI Kit. Main UI includes project navigation and management, mode switching, search, menus, dialogs, notifications, shared window controls, and the bottom control bar. Settings includes General, appearance, feature visibility, keyboard settings, Chats settings, licenses, and agent management. Include agent creation and editing and project-management forms.

**R2.** Preserve existing capabilities and underlying behavior in these retained areas, except for the explicit excluded-mode removal below or a later user-approved change. This is a UI rebuild, not a change to agent execution, protocol behavior, project ownership, conversation lifecycle, or storage semantics. Project and agent management may change their presentation entirely while retaining their operations and safeguards.

**R3.** Preserve the general Main UI layout: top projects bar, modes bar, left/central/right panel arrangement, and bottom control bar. Preserve applicable panel visibility and resizing behavior. Controls, styling, spacing, and local arrangements may change within that structure.

**R4.** A proposed structural layout change requires a visible comparison before approval. During later design work, show the requester a quick mockup, prototype, or HTML visualization of the proposed alternative, explain the concrete improvement and tradeoffs, and obtain explicit approval before adopting it. A verbal proposal alone is insufficient. Until approval, R3 remains binding.

**R5.** Use standard GPUI Kit controls wherever they cover the required behavior. Custom UI is permitted only for demonstrated component gaps or genuinely app-specific content. For each existing custom control proposed for retention because a standard replacement cannot preserve its behavior, explicitly tell the requester which control, the closest standard alternative, the verified gap, and the functionality that replacement would lose. The requester may approve that loss to favor standard components; no loss is approved by this scope. Do not silently retain a custom control or silently reduce functionality.

**R6.** Use Ghostex only as a source of examples. Check relevant choices against current Kit documentation/source and Adeline's requirements before adopting them. Comparisons must state factual benefits, constraints, and tradeoffs rather than assume Ghostex is correct. Its features, dependency patches, and conventions are not automatically requirements for Adeline.

### Excluded modes and compatibility

**R7.** Remove the working content, mode-specific actions, and mode-specific settings UI of Docs, Workflows, Services, Groupchats, Issues, and Whiteboard. Retain an empty destination for each, its identity in navigation, and its feature toggle. Enabled modes remain switchable; disabled modes follow existing feature-visibility behavior. Chats remains available. Shared controls must not offer removed mode-specific operations.

**R8.** Leave existing saved files and preferences belonging to those modes intact, but ignore their content-specific preferences and data in the running app. Existing settings containing those preferences remain loadable. Saving an unrelated setting must not remove or change the retained legacy preference values. The mode feature toggles remain active preferences, not ignored legacy preferences.

**R9.** Do not create or recreate obsolete mode-specific preferences or saved mode content when absent, including when creating settings for a new user. New settings retain the mode feature toggles needed by R7, but omit the removed modes' content and panel preferences. Existing retained legacy values must not be used as a reason to restore the removed UI.

**R10.** Demo mode shows the same empty excluded-mode destinations as normal operation. Preserve the retained shell and Chats demo functionality and its isolation from real agent definitions, projects, and conversation history. Do not keep the removed simulated modes available only in demo mode.

### Themes and appearance

**R11.** Preserve compatibility with existing bundled and user theme files without requiring users to edit or convert them. Preserve theme discovery, saved selection, and the use of each theme's palette throughout rebuilt surfaces, including menus, overlays, dialogs, and supporting windows. A different control layout or geometry does not require pixel-for-pixel reproduction of the old UI.

**R12.** Preserve separate interface/code font preferences and sizes, installed-font selection, bundled defaults, and missing-font fallback without discarding the saved choice. Retain the existing 10–24 size range and immediate persistence. Typography changes must leave wrapping, scrolling, and controls usable.

## Constraints and quality requirements

**R13.** Apply the GPUI Kit Design Guides as the visible quality baseline. Require consistent control sizes and spacing, aligned labels and fields, clear reading hierarchy, and distinct hover, focus, selection, disabled, loading, error, and destructive states where applicable. Use the existing theme palettes through semantic roles. Appearance improvements must remain usable in light, dark, and custom themes.

**R14.** All in-scope commands must be operable without a mouse. Focus must be visible and follow a logical order. Menus must support keyboard selection. Escape must dismiss the topmost dismissible surface and restore focus to its trigger or the next logical target. Existing configured shortcuts remain effective. Text fields retain selection, clipboard, multiline input where applicable, and IME behavior. Controls have accessible names; essential actions and status must not depend only on hover or color.

**R15.** Retain usability when resizing the window, resizing panels, changing font sizes, and using platform display scaling. Essential controls must remain reachable without clipping; scrolling belongs to the region containing the overflowing content. Streaming and long transcripts must remain readable and navigable without disrupting composition or keyboard operation. This scope introduces no new numerical performance targets.

**R16.** Preserve Windows, macOS, and Linux support, including X11 and Wayland. Preserve platform-appropriate shortcuts and native window behavior, including usable dragging, resizing, and window controls. Do not infer cross-platform correctness from Ghostex or from a single-platform result.

## Failure and edge cases

**R17.** Preserve existing Chats success, waiting, failure, and recovery outcomes. This includes formatted streaming responses, tool visibility and expansion, permission requests, cancellation with partial output retained, configured retry limits, session restoration and replacement-session consent, saved history without a running harness, and transcript-storage failure handling. Hidden tool output must not hide required permission decisions. Switching chats, projects, or modes must not inadvertently cancel work or lose retained conversation state.

**R18.** Preserve existing project and agent validation, destructive-action safeguards, unsaved-edit choices, external-edit conflict choices, and graceful process shutdown. Preserve existing data ownership and storage locations; project deletion must not delete its working directory. Redesigning a form or replacing its controls must not broaden permissions, bypass consent, or lose saved data.

**R19.** Preserve theme/settings failure safeguards. Report invalid theme files while keeping valid choices available; preserve the existing usable startup fallback for a missing or invalid selected theme. Do not overwrite user-edited theme files. Invalid settings must not be silently replaced while saving an unrelated setting. Ignored legacy preferences under R8 must not make an otherwise valid existing configuration unusable.

## Boundaries

### Included

- Main UI, Settings, Chats, and their supporting project/agent management flows.
- Empty destinations and feature toggles for the six excluded modes.
- Compatibility with retained user data, themes, and applicable preferences.
- User decision gates for structural layout exceptions and custom-control replacement gaps.

### Explicit exclusions

- Rebuilding or retaining functional Docs, Workflows, Services, Groupchats, Issues, or Whiteboard content.
- Their mode-specific settings UI and operations.
- New agent/backend capabilities or changes to retained business behavior solely because the UI is rebuilt.
- Importing Ghostex features simply because they appear in the reference app.

This document ends at scope. Component selection details, UI/UX design, architecture, program design, tickets, implementation, test plans, and delivery scheduling belong to later work. The requested visual comparisons and replacement-gap reports are requirements for that later work, not claims that prototypes or an exhaustive component audit already exist.

### Rejected ideas

- Keeping the old functional excluded-mode views within the rebuilt shell.
- Automatically treating Ghostex's implementation as the preferred approach.
- Unapproved structural layout changes based only on a claim that an alternative is superior.
- Silently sacrificing existing functionality to replace a custom control.
- Seeding obsolete mode-specific settings for new users.

## Domain, data, and interfaces

A project remains the owner of its conversations and their working-directory context. A conversation retains its saved agent/session configuration and history. Editing an agent definition continues to affect new conversations rather than silently rewriting existing conversation configuration, as documented in `README.md`, "Agents".

Feature toggles control whether the optional empty mode destinations are available. They are distinct from the removed modes' retained-but-ignored content and panel preferences. Preserving those legacy values means preserving their stored meaning and values during unrelated settings changes; it does not require keeping their old controls active.

Required external interactions remain the existing agent/harness protocol, local settings/theme/agent/project/history files, installed fonts, and desktop window/input facilities. No new remote service, synchronization system, or Ghostex runtime integration is requested.

## Acceptance criteria

The following criteria express the agreed requirements. Confirmation of this document accepts these criteria; they are outcomes, not an execution plan.

| ID | Requirements | Observable acceptance condition |
| --- | --- | --- |
| AC1 | R1 | The rebuilt shell, retained Settings pages, Chats, and project/agent creation and management surfaces use Kit-based UI and remain reachable through their normal entry points. |
| AC2 | R2 | Existing retained project, agent, and chat workflows produce the same underlying results and use existing saved data; any functional exception has explicit user approval. |
| AC3 | R3 | The default rebuilt shell retains the top projects bar, modes bar, three-region panel arrangement, bottom control bar, and applicable panel visibility/resizing operations. |
| AC4 | R4 | Every adopted structural exception has a shown mockup/prototype/HTML visualization, a concrete comparison, and explicit requester approval. Without approval the baseline layout remains. |
| AC5 | R5 | Standard Kit controls replace equivalent custom controls. Each retained custom-control gap is disclosed by name with a verified limitation and replacement tradeoff; any functionality loss has explicit approval. |
| AC6 | R6 | Each Ghostex-derived choice is justified against current Kit capabilities and Adeline's requirements; reference-only features and patches are not adopted by assumption. |
| AC7 | R7 | Each of the six optional modes can be enabled, selected as an empty destination, and disabled. Its old content, actions, and mode-specific settings are unavailable. Chats remains accessible. |
| AC8 | R8 | Existing configurations containing excluded-mode preferences load; those values and saved files remain intact after unrelated settings changes, have no effect on empty views, and do not disable functioning feature toggles. |
| AC9 | R9 | A new user's settings contain the optional-mode feature toggles but no obsolete mode-specific content/panel preferences. Absent obsolete preferences and content are not regenerated during startup or later settings saves. |
| AC10 | R10 | Demo mode exposes empty excluded-mode destinations, still supports retained shell/Chats demonstrations, and does not modify real user definitions or history. |
| AC11 | R11 | Existing bundled and custom YAML themes work unchanged, remain selectable across restarts, and apply consistently to the rebuilt main and supporting surfaces. |
| AC12 | R12 | Independent interface/code font and size choices persist and apply; missing fonts use the existing fallback without erasing the choice; the existing size range remains usable. |
| AC13 | R13 | Retained surfaces meet the stated guide-based hierarchy, alignment, spacing, interaction-state, and theme requirements across normal, empty, loading, and error states where applicable. |
| AC14 | R14 | A keyboard-only user can navigate and operate all in-scope commands, use configured shortcuts, edit text including IME input, and dismiss overlays with correct focus restoration. Essential actions do not require hover. |
| AC15 | R15 | Window/panel resizing, supported typography changes, display scaling, streaming, and long transcripts keep essential content and controls reachable and leave typing, focus, and scrolling usable. |
| AC16 | R16 | Retained functionality remains supported on Windows, macOS, and Linux with X11 and Wayland, with platform-appropriate shortcuts and functional native window interactions. |
| AC17 | R17 | Streaming, tool display, permissions, Stop, retries, recovery, history replay, and storage-failure states retain their documented outcomes; navigation does not inadvertently stop agents or discard conversation state. |
| AC18 | R18 | Invalid input is rejected with useful feedback; unsaved/conflicting edits retain existing choices; destructive operations retain consent and data boundaries; shutdown retains existing graceful/force-stop behavior. |
| AC19 | R19 | Invalid/missing themes expose errors and preserve usable fallback behavior; user theme edits and malformed settings are not overwritten; ignored legacy preferences do not prevent otherwise valid configuration from loading. |

## Decisions and rationale

- Q1–Q3: Maintenance, custom-control styling, keyboard behavior, and compatibility are the drivers. Preserve capabilities and data while improving presentation and interaction.
- Q4, Q8–Q9: Replace excluded-mode functionality with empty navigable views. Retain and ignore existing legacy data/preferences; do not seed missing obsolete preferences. Apply the same boundary to demo mode.
- Q5, Q10: Preserve existing themes, typography preferences, and the named main layout. The requester decides layout exceptions after seeing a visual alternative.
- Q6: Include the supporting project and agent management flows, with freedom to change their presentation.
- Q7: Ghostex supplies examples, not assumed correct answers. Evaluate relevant alternatives factually.
- Q11: Prefer standard controls. Report verified replacement gaps to the requester, who may choose an explicit functionality tradeoff later.
- Q12–Q14: Accept the keyboard baseline, existing platform coverage, and guide-based appearance/usability requirements.

## Open questions

None. Later component-gap and layout-exception decisions follow R4 and R5; no exception or functionality loss is currently approved.
