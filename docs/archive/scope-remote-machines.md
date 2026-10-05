Status: Implemented in v0.1.5. This scope is history. Where it and the code differ, the code is right.

# Remote machines

## Purpose and context

People who keep projects on more than one computer can't see or drive their agents on another machine today. One Adeline client should show and drive projects and conversations from several machines at once. Example: agents run on a Windows desktop at home. On a MacBook elsewhere, the user checks both machines in the machine selector. The project bar shows the desktop's projects next to the MacBook's. The user sends a prompt in a desktop conversation, answers its permission request, and watches the reply stream in.

Each conversation stays on the machine whose engine runs it. Clients show and drive it remotely. Nothing is copied between machines.

Current behavior:

- The UI and the engine are separate processes. The engine owns agent processes, conversation state and storage, and outlives the UI (`docs/architecture.md`, `engine::main`).
- The UI connects to one engine through a same-user named pipe or Unix socket, one JSON object per line (`src/ipc.rs`: `connect`, `Listener`, `connect_or_start`). Message types are in `src/protocol.rs`, which states that nothing in it assumes the client shares the engine's machine.
- On connect the engine sends a full `Snapshot`, then `Delta`s with no sequence numbers (`protocol::EngineMessage`, `client.rs`: `run_io`).
- One engine already serves several clients and sends every change to all of them (`Engine::subscribers`).
- Projects, conversations and agent definitions are stored under the engine host's `~/.config/adeline` (`storage.rs`, `agents.rs`, `config::directory`). A project's `directory` is a path on that host (`data::Config`).
- The engine exits 60 s after its last client leaves, unless it runs as a daemon or `keep_running` is set (`engine.rs`: `IDLE_EXIT`, `Engine::daemon`). `keep_running` is the Engine settings page (`settings.rs`, `KEEP_RUNNING`).
- A placeholder single-choice machine selector sits in the title bar behind `features.machine_selector`, off by default (`config.rs`, `project_bar.rs`, `views.rs` "machines", `interaction.rs` `Action::Machine`). Outside demo mode it lists only "Local machine". Demo mode lists the fake machines in `main.rs` `MACHINES` (Nexus, Matrix, Vortex).
- "Open folder…", the project dialog, and "Add a file / Add a directory" use the native picker, which sees only the client's disk (`interaction.rs` `Action::OpenFolder`, `Action::AddFile`, `Action::AddDirectory`; `project_ui.rs`). `open_folder` (`project_bar.rs`) matches existing projects by directory alone.
- `ui_state.rs` remembers when each project was last opened, by project ID, on the client only.
- Settings groups are General (Features, Appearance, Keymap), Modes, Licenses, Agents and Engine (`settings.rs` `GROUPS`).
- Releases are on GitHub (`https://github.com/aseeon/adeline/releases`). `v0.1.0` ships `windows-x86_64`, `macos-arm64` and `linux-x86_64` zips.

## Requirements

### Machines and the selector

- R1. A machine is either the local machine or a saved remote machine. A remote machine has a display name and an ordered list of one or more SSH destinations. Each destination is `user@host[:port]` or a `Host` alias from `~/.ssh/config`.
- R2. The machine selector lists the local machine and every saved remote machine, each with a checkbox. Any combination can be checked, as long as at least one stays checked. The local machine is an ordinary entry and can be unchecked.
- R3. The project bar and the projects menu show the projects of every checked machine.
- R4. Checking a machine connects to it. Unchecking it disconnects it. An unchecked machine shows no projects, raises no notifications and shows no attention dots.
- R5. Each client remembers which machines are checked across restarts. On first start, or when only the local machine exists, the local machine is checked.
- R6. The selector appears in the title bar once at least one remote machine is saved. With no remote machines it isn't shown. The `features.machine_selector` flag and its settings toggle are removed.
- R7. Each selector entry shows its machine's state: connected, connecting, disconnected, sign-in failed, upgrade needed, local update needed, or unsupported. The error behind the state can be read from the entry, on hover or click.
- R8. When more than one machine is checked, project tabs and projects-menu rows show the machine's name. With one machine checked they look as they do today.
- R9. Saved machines and their destinations are stored only on the client where they were added. Other clients don't see them.
- R10. Removing a machine only forgets it on this client. Its engine, projects and conversations stay on that machine, untouched. If it runs all the time, its agents keep running.

### Driving remote projects

- R11. A connected remote machine's projects and conversations can do everything a local one can: open, send, stop, retry, fork, answer permissions, switch settings, complete, archive, mark read, rename, and delete. Each runs on that machine's engine against that machine's files, through that machine's agent definitions.
- R12. Two clients on the same conversation behave like two local windows today. A reply streams to both, and a permission answered on one closes on the other.
- R13. Adding a project on a remote machine lets the user browse that machine's folders inside Adeline.
- R14. With more than one machine checked, "Open folder…" first asks which machine, defaulting to the last one used, then browses that machine's folders. Whether a folder is already an open project is decided by machine and directory together, so the same path on two machines is two projects.
- R15. In a conversation on a remote project, "Add a file" and "Add a directory" browse that machine's files.
- R16. What the client remembers for itself about projects (for example last opened, `ui_state.rs`) is kept per machine and project.

### Settings per machine

- R17. The Settings window keeps its current groups. A machine dropdown at the bottom of the left panel chooses the local machine or a saved remote machine. It defaults to the local machine.
- R18. Choosing a remote machine switches the settings list to that machine's own groups, Agents and Engine. General, Modes and Licenses stay client-only and are hidden while a remote machine is chosen. Changes there apply to that machine's engine.
- R19. Agent definitions belong to each machine. An agent added for a machine is available only for that machine's projects.
- R20. Each machine's engine has its own Engine settings, including the existing choice between stopping when idle and running all the time.
- R21. The dropdown ends with "Add machine…" and "Manage machines…". They open a machines page where machines are added, edited and removed, and their destinations are added, removed and reordered.

### Connecting

- R22. Remote machines are reached only through the system `ssh`, honoring the user's `~/.ssh/config`, keys and SSH agent. Adeline opens no network port on any machine. Only someone who can SSH in as the engine's OS user can reach that engine.
- R23. Password, passphrase and other prompts from `ssh` appear in an Adeline dialog. Adeline never stores passwords or passphrases.
- R24. An unknown host key is shown in an Adeline dialog for the user to accept or reject. Adeline never accepts one on its own. A changed host key is refused, and the `ssh` warning is shown.
- R25. Destinations are tried in their saved order on every connect and reconnect. The first one that works is used. A working connection doesn't switch to another destination.
- R26. Every engine has a stable identity. If a destination reaches an engine other than the one saved for that machine, that destination is refused, the message names it, and the next destination is tried.
- R27. Adding a machine whose engine is already saved under another name is refused, and the message names the existing machine. Its destination can be added to that machine instead.
- R28. When the remote machine's engine isn't running, connecting starts it. While connected, and afterwards, it follows that machine's idle-or-always-on setting.

### Installing and versions

- R29. If Adeline is missing on the remote machine, or older than the client, the client installs its own version there over SSH from the GitHub release for that version and the remote's OS and CPU type.
- R30. If no release exists for the client's version, such as an unreleased or development build, and the remote has the same OS and CPU type, the client uploads its own binary. Otherwise it shows an error naming the missing version and platform.
- R31. Adeline never downgrades a remote machine. If the remote runs a newer Adeline, the machine is not connected and shows "local update needed", asking the user to update Adeline on this client.
- R32. Upgrading a running remote engine restarts it, which stops its agents. Before upgrading, Adeline asks, showing how many conversations are active there: for example "Matrix runs Adeline 0.1.2 with 2 active conversations. Upgrade now (stops them) / Later". Choosing "Later" leaves the machine disconnected with "upgrade needed".
- R33. Supported remote hosts are Windows x86-64, macOS arm64 and Linux x86-64, the platforms the release ships. Any other host shows "unsupported" with a message naming its platform, for example "No Adeline build for linux-aarch64".

### Failure and edge cases

- R34. When a checked machine drops, its projects and conversations stay visible as they were last received, marked disconnected and read-only. Adeline reconnects in the background with increasing delays. Other machines are unaffected.
- R35. A checked machine that can't be reached at startup shows as unreachable, with a retry option, and the other machines load normally.
- R36. After a sign-in failure (wrong password, rejected key), Adeline stops retrying that machine and shows "sign-in failed" until the user retries it by hand. Timeouts and network failures keep retrying with increasing delays.
- R37. After reconnecting, a conversation shows exactly what the engine recorded. Text and tool activity from while the client was disconnected appear, nothing is duplicated, and an ongoing reply keeps streaming.
- R38. A reconnect after a short drop sends only what the client missed, not the full history.
- R39. At 50 ms of network latency, text missed during a drop appears within 2 s of the connection coming back.

### Demo mode

- R40. In `--demo`, demo projects are spread across the demo machines Nexus, Matrix and Vortex. Checking machines, machine labels, the settings machine dropdown and the disconnected look all work with no SSH. One demo machine is shown disconnected.

## Boundaries

### Excluded

- Mobile and web clients. They are a separate scope.
- History copied to other machines, or readable while its machine is offline. Disconnected machines show only what is already in the client's memory (R34).
- A hosted relay, accounts, QR pairing, or Adeline's own way across NAT. Getting from one network to another is up to the user's SSH setup (Tailscale, VPN, port forwarding, several destinations).
- Remote machines from the command line. `adeline` CLI requests keep talking only to the local engine.
- Builds for platforms the release doesn't ship (R33). Adding them is a release-pipeline change.
- Sharing the saved machine list between clients (R9).

### Interactions with existing functionality

- The local engine keeps working as today: same pipe or socket, same start and idle behavior.
- With only the local machine, the window looks and behaves as today. No selector, no labels (R6, R8).
- Existing project and conversation data on every machine stays readable.

### Rejected ideas

- Syncing conversations to every machine with a CRDT, as Zeron does.
- A connection that switches back to a higher-priority destination while connected (R25).
- Upgrading remote engines without asking (R32).
- Only one destination per machine (R1, R25).
- Agent definitions shared across machines (R19).
- Storing SSH passwords in the OS keychain (R23).
- A CLI `--machine` option.
- Keeping the selector behind a feature flag (R6).

## Domain and data

- **Machine:** the local machine, or a remote machine saved on this client. A remote machine has a display name, an ordered list of SSH destinations, and the identity of the engine it was first connected to.
- **Destination:** one way to reach a machine over SSH.
- **Engine identity:** a stable identifier each engine keeps for its lifetime. It ties destinations to one machine (R26, R27).
- **Checked machines:** the client's own choice of which machines to connect and show (R5).
- **Owned by each engine's machine:** projects, conversations, transcripts, agent definitions and Engine settings.
- **Owned by each client:** saved machines and destinations, checked machines, per-project UI memory (R16), and the General, Modes and Licenses settings.
- **Machine states:** connecting, connected, disconnected, sign-in failed, upgrade needed, local update needed, unsupported (R7).

## Interfaces and dependencies

- The system `ssh` client on the client machine. An SSH server on each remote machine, which for Windows is the OpenSSH Server feature.
- GitHub releases at `https://github.com/aseeon/adeline/releases` for the client's version and the remote's platform (R29).
- Affected parts: engine identity and the client connection (`ipc.rs`, `client.rs`, `protocol.rs`), the engine's catch-up after reconnect (R38), the machine selector (`project_bar.rs`, `views.rs`, `interaction.rs`), the project bar and menu, the project dialog and "Open folder…", file attachments, Settings (`settings.rs`), `ui_state.rs`, demo data (`assets/workspace.json`, `main.rs` `MACHINES`), and the UI checks in `scripts/engine-check/`.

## Acceptance criteria

- AC1 (R1, R21). On the machines page, a user adds "Desktop" with destinations `desktop.lan` and `me@home.example.com:2222`, reorders them, edits the name, and removes one. A `Host` alias from `~/.ssh/config` is accepted as a destination.
- AC2 (R2, R3). With the local machine and two remote machines checked, the project bar and the projects menu list projects from all three. Unchecking the last checked machine is refused.
- AC3 (R4). Unchecking a machine closes its connection, and its projects, notifications and attention dots disappear. Checking it again reconnects and shows them.
- AC4 (R5). After restarting the client, the same machines are checked. A fresh install with no remote machines starts with the local machine checked.
- AC5 (R6). With no remote machines saved, the title bar has no machine selector and Settings has no "Machine selector" feature toggle. Adding a remote machine makes the selector appear.
- AC6 (R7). Each state in R7 can be produced and shows on the selector entry, with its error readable from the entry.
- AC7 (R8). Two machines that each have a project called "app" show two tabs labeled with their machine names. With one machine checked, tabs show no machine label.
- AC8 (R9). A machine added on client A doesn't appear on client B.
- AC9 (R10). Removing a running always-on machine leaves its engine running, with agents still running and all data on that machine intact. Adding it back shows the same projects.
- AC10 (R11). On a remote project, each action listed in R11 has the same effect on the remote engine as it has locally, and agents run in the remote project's directory on the remote machine.
- AC11 (R12). With two clients on one remote conversation, a reply streams on both, and answering a permission on one closes the request on the other.
- AC12 (R13). Creating a project on a remote machine opens a folder browser showing that machine's folders, and the project uses the chosen remote path.
- AC13 (R14). With two machines checked, "Open folder…" asks for a machine, defaulting to the last one used. Opening `C:\code\app` on each of two machines creates two projects. Opening the same path on the same machine again switches to the existing project.
- AC14 (R15). "Add a file" in a remote conversation browses the remote machine, and the attached path is a path on that machine.
- AC15 (R16). Opening a project on one machine doesn't change the last-opened record of a project with the same ID on another machine.
- AC16 (R17, R18). The Settings left panel has a machine dropdown defaulting to the local machine. Choosing a remote machine shows only Agents and Engine for that machine. Switching back shows every group again.
- AC17 (R19). An agent added for remote machine M is offered for new conversations in M's projects and not in other machines' projects.
- AC18 (R20, R28). With M set to stop when idle, M's engine exits after its last client leaves and no agent is working. With M set to always on, it keeps running. Connecting to M when its engine isn't running starts it.
- AC19 (R22). No Adeline process on any machine listens on a network port. A user who can't SSH in as the engine's OS user can't reach the engine.
- AC20 (R23). Connecting to a password-only host shows a password dialog in Adeline, and the connection succeeds. Nothing in Adeline's files or the OS keychain stores the password afterwards.
- AC21 (R24). First connect to a host with an unknown key shows the key in an Adeline dialog, and rejecting it aborts. A changed host key is refused with the ssh warning shown.
- AC22 (R25). With `desktop.lan` first and `home.example.com:2222` second: on the home network the first is used, and elsewhere the second is used after the first fails. While connected through the second, the client doesn't switch when the first becomes reachable.
- AC23 (R26). A destination that reaches a different engine is refused with a message naming it, and the next destination is tried.
- AC24 (R27). Adding a machine whose engine is already saved as "Desktop" is refused with a message naming "Desktop".
- AC25 (R29). Connecting to a supported host with no Adeline installs the client's version from the matching release, and the machine connects. A host with an older version is upgraded the same way, after R32's prompt.
- AC26 (R30). With an unreleased client version: a remote of the same OS and CPU type receives the client's own binary and connects. A remote of another platform shows an error naming the version and platform.
- AC27 (R31). A remote running a newer Adeline isn't changed. The machine shows "local update needed".
- AC28 (R32). Upgrading a remote with active conversations first shows the prompt with the count. "Later" leaves the engine and its agents running and the machine in "upgrade needed".
- AC29 (R33). A linux-aarch64 or Intel macOS host shows "unsupported" with its platform named.
- AC30 (R34). Cutting the network to a checked machine keeps its projects and open conversation visible, marked disconnected and read-only, while other machines keep working. Restoring the network reconnects with no user action.
- AC31 (R35). Starting the client with one checked machine unreachable shows it as unreachable with a retry option, and the other machines load.
- AC32 (R36). A wrong password stops retries and shows "sign-in failed" until a manual retry. A timeout keeps retrying with growing delays.
- AC33 (R37). Text and tool calls the agent produced while the client was disconnected appear after reconnect, exactly once and in order, and the ongoing reply keeps streaming.
- AC34 (R38). After a short drop on a conversation with long history, the engine sends only the missed events, not a full snapshot.
- AC35 (R39). At 50 ms latency, missed text appears within 2 s of the connection returning.
- AC36 (R40). In `--demo`, projects appear under Nexus, Matrix and Vortex. Checking and unchecking, machine labels and the settings dropdown work, and one machine is shown disconnected. `scripts/engine-check/ui_check.py` covers these screens.

## Decisions and rationale

- Each machine keeps its own conversations, and the client shows several machines together. This is what Zed, T3 Code, Paseo and Ghostex do. A conversation can't continue on another machine, because its files, agent process and agent session state stay where it ran.
- SSH is the only way to connect. It brings authentication, encryption and host trust without a hosted service, and lets users reuse their own network setup.
- Several destinations per machine, because not everyone uses Tailscale, and many have one address at home and another outside.
- Install and upgrade only upward, with the client's exact version, so a client and an engine never run different protocol versions. Upgrades ask first because they stop running agents.
- Agent definitions and Engine settings stay with each machine, because an agent points to an executable installed on that machine.
- Settings for other machines sit behind a dropdown in the existing Settings window, as Paseo does it, instead of a new top-level group.

## Open questions

None.
