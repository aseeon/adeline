Status: Implemented in v0.1.6. This scope is history. Where it and the code differ, the code is right.

# Headless build

## Purpose and context

Remote machines (`docs/archive/scope-remote-machines.md`) get Adeline installed over SSH. Today that install is the full desktop build. On a small SSH server, such as a VPS or a home box with no desktop, the full Linux build may not even start `adeline bridge`, because it links desktop (X11/Wayland) libraries the server lacks. Connecting then fails, or the user has to install desktop libraries by hand.

Users are people who connect from a laptop to such servers, and anyone who installs the engine on a server by hand.

Intended outcome: every release also ships a headless build per platform. It has no window and no desktop or GPU libraries, and it runs `engine`, `bridge` and the engine commands. The client installs it on every remote machine, so a bare server just works. ARM Linux servers become supported.

Current behavior:

- One binary does everything. `main` in `src/main.rs` dispatches `engine`, `bridge` and `--version`, and opens the GPUI window for anything else.
- The engine side barely depends on GPUI. The links are a GPUI global (`Catalog`) and `init` in `src/harness.rs`, the UI helpers `text_pixels` and `bind_keys` in `src/config.rs`, and `Section` and `short` defined in `src/main.rs`.
- The `build` job in `.github/workflows/ci.yml` uploads raw binaries as `adeline-<OS>` artifacts. The release zips (`adeline-windows-x86_64.zip`, `adeline-macos-arm64.zip`, `adeline-linux-x86_64.zip`) are made by hand.
- Remote installs download `adeline-<platform>.zip` (`remote::download` in `src/remote.rs`). With no release and a matching platform, `remote::binary` sends the client's own executable. `remote::SUPPORTED` lists `windows-x86_64`, `macos-arm64` and `linux-x86_64`; any other platform shows "unsupported".
- The client decides what to install from the plain `~/.adeline/bin/version` file and requires the engine's reported version to equal its own (`remote::attempt`).

## Requirements

### The headless build

- R1. Each release has a headless build for Windows x86_64, macOS arm64, Linux x86_64 and Linux aarch64, as `adeline-headless-<platform>.zip`: `adeline-headless-windows-x86_64.zip`, `adeline-headless-macos-arm64.zip`, `adeline-headless-linux-x86_64.zip` and `adeline-headless-linux-aarch64.zip`.
- R2. Each zip holds one executable named `adeline` (`adeline.exe` on Windows), the same name as the full build.
- R3. The headless build does everything the full build does except the window: `adeline engine` with `status`, `start [--daemon]` and `stop`, `adeline bridge`, and `adeline --version`. The engine and the bridge behave as in the full build.
- R4. The headless build needs no desktop, windowing or GPU libraries on any platform.
- R5. Both Linux headless builds are static musl builds that run on any Linux distribution of their architecture, including Alpine and distributions with an old glibc.
- R6. Run with no command, an unknown command, or `--demo`, the headless build prints a short usage line naming `engine`, `bridge` and `--version`, and exits with code 2.
- R7. On Windows the headless build is a console program: its commands print to the terminal and exit like any command-line tool. Started over SSH or in the background, it opens no visible window.
- R8. `adeline --version` prints the version followed by ` (headless)`, for example `0.1.6 (headless)`. The full build keeps printing only the version.
- R9. The version recorded in `~/.adeline/bin/version` and the version the engine reports to clients stay the plain version, the same for both builds, so a full client and a headless engine of the same version connect.
- R10. The Engine settings page shows a "Build" line, `headless` or `full`, for the machine selected in the Settings machine dropdown.

### Remote installs

- R11. When the client installs or upgrades Adeline on a remote machine, it installs the headless build for that machine's platform, from `adeline-headless-<platform>.zip` of the client's own version, on every platform.
- R12. When the client's version has no release and the remote machine's platform matches the client's, the client keeps sending its own full executable, as today.
- R13. Linux aarch64 remote machines are supported. When the client's version has no release, an aarch64 machine shows an error naming the missing version and platform, as for any platform without a build.
- R14. A remote machine that already runs the full build at the client's version is left as it is. The headless build replaces it only at the next install or upgrade.

### CI and documentation

- R15. Every CI run builds the headless binary for all four platforms and uploads each as a ready-to-release `adeline-headless-<platform>.zip`.
- R16. CI runs clippy and the tests on the headless configuration too, so it can't break unnoticed.
- R17. CI runs each Linux headless binary in a minimal container with no desktop libraries (Alpine; aarch64 through emulation). There `adeline --version` must print the headless marker, and `adeline engine start`, `adeline engine status` and `adeline engine stop` must work.
- R18. The README's download section briefly says what the headless zips are for and which to take for a server. The release notes list the headless zips.

## Boundaries

### Excluded

- Changes to the full desktop builds and their hand-made zips.
- Intel macOS and Windows on ARM builds.
- Building or fetching an ARM Linux build when no release exists (R13).

### Interactions with existing functionality

- This supersedes the remote-machines scope's platform list: remote installs now cover `linux-aarch64` too, and install the headless build instead of the full one.
- Local use is unchanged: the desktop app keeps starting its local engine from its own full executable.
- The fallback that sends the client's own executable (R12) still installs a full build. The headless build replaces it at the next install or upgrade with a release.

### Rejected ideas

- Splitting the code into separate crates. A build option in the one crate gives the same binary.
- Keeping one binary and loading desktop libraries only when needed. GPUI links them when it's built.
- A glibc build for Linux headless.
- Swapping an existing same-version full install for the headless build on connect (R14).
- A plain `--version` without the marker.

## Domain and data

- **Full build:** today's desktop app with its window, also able to run `engine` and `bridge`.
- **Headless build:** the same program without the window or desktop libraries.
- **Build kind:** full or headless. Shown by `--version` and the Engine settings page. It never changes the plain version used to compare installs.

## Interfaces and dependencies

- `.github/workflows/ci.yml`: new headless build, check and container smoke jobs, and zip artifacts.
- `src/remote.rs`: download name, platform list, install source.
- `src/main.rs`: command dispatch and usage line for the headless build.
- The engine status reaching the client, so Settings can show the build kind.
- GitHub releases carry the headless zips next to the full ones.
- `README.md` download section and the release notes.

## Acceptance criteria

- AC1 (R1, R2, R15). A CI run uploads `adeline-headless-windows-x86_64.zip`, `adeline-headless-macos-arm64.zip`, `adeline-headless-linux-x86_64.zip` and `adeline-headless-linux-aarch64.zip`, each holding one executable named `adeline` or `adeline.exe`.
- AC2 (R3, R4, R5, R17). In an Alpine container with no desktop libraries, both Linux headless binaries run `--version`, `engine start`, `engine status` and `engine stop` successfully.
- AC3 (R3). Over the fake-SSH checks, a remote machine running the headless build connects, runs a prompt through its engine, and reconnects after a drop, as with the full build.
- AC4 (R6). `adeline`, `adeline --demo` and `adeline frobnicate` from the headless build print the usage line and exit with code 2, and no window opens.
- AC5 (R7). On Windows, `adeline engine status` from the headless build prints to the terminal it runs in, and starting it over SSH or in the background shows no window.
- AC6 (R8, R9). The headless `adeline --version` prints `<version> (headless)` and the full build prints `<version>`. After a headless install, `~/.adeline/bin/version` holds the plain version, and a full client of that version connects without an upgrade or update prompt.
- AC7 (R10). With a headless remote machine selected in the Settings machine dropdown, the Engine page shows "Build: headless". With the local machine selected, it shows "Build: full".
- AC8 (R11). Installing on a machine with no Adeline, or with an older one, downloads `adeline-headless-<platform>.zip` of the client's version, and that machine's `adeline --version` then shows the headless marker.
- AC9 (R12). With an unreleased client version, a remote of the client's platform receives the client's own full executable and connects.
- AC10 (R13). A Linux aarch64 machine installs the headless build from a release and connects. With an unreleased client version, it shows an error naming the version and `linux-aarch64`.
- AC11 (R14). A machine that already runs the full build at the client's version connects without being reinstalled, and its engine keeps running.
- AC12 (R16). CI fails when the headless configuration doesn't compile, fails clippy, or fails its tests.
- AC13 (R18). The README's download section says what the headless zips are for in a sentence or two, and the release notes for the version that ships them list them.

## Decisions and rationale

- One crate with a build option rather than a split, because the engine side barely depends on GPUI.
- Headless on every shipped platform, so remote installs follow one rule and download less.
- Static musl for Linux, because the point is running on minimal servers.
- Linux aarch64 added, because cheap ARM machines are typical small SSH servers.
- Same executable name in the zips, so the remote install path `~/.adeline/bin/adeline` doesn't change.
- The headless marker is display-only, because installs and connections compare plain versions.
- The Windows headless build is a console program, because it's only used from terminals, SSH and the background.

## Open questions

None.
