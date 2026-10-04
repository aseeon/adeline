<p align="center">
  <img src="assets/adeline-banner.png" alt="Adeline" />
</p>

<p align="center">
  <strong>A native desktop app for running coding agents across your projects.</strong><br />
  Chat with Claude Code, Codex, OpenCode, oh-my-pi, and any other ACP agent in one window.
</p>

<p align="center">
  <a href="https://github.com/aseeon/adeline/actions/workflows/ci.yml"><img src="https://github.com/aseeon/adeline/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/aseeon/adeline" alt="License" /></a>
  <img src="https://img.shields.io/badge/platforms-Windows%20%C2%B7%20macOS%20%C2%B7%20Linux-blue" alt="Platforms: Windows, macOS, Linux" />
</p>

<p align="center">
  <img src="assets/adeline-screenshot.png" alt="Adeline with a project's chats on the left, a Claude Code conversation in the middle, and the agent's activity on the right" width="960" />
</p>

Adeline is an agentic development environment written in Rust. It keeps your projects, agents, and conversations in one fast native app. Agents keep working after you close the window, and every conversation is saved to disk.

> [!NOTE]
> Adeline is in early development. Expect missing features and breaking changes between versions.

## Features

- **Any ACP agent.** Pick a harness from the [Agent Client Protocol registry](https://github.com/agentclientprotocol/registry), or point Adeline at any command that speaks ACP.
- **Saved agents.** Give each agent a name, harness, model, effort level, and default permission mode, then reuse it across projects.
- **Projects and conversations.** Each project has its own working directory and its own list of chats. Complete or archive a chat to close its agent and keep its history.
- **Switch models mid-chat.** Change the model or effort level of a running conversation without touching the agent's defaults.
- **Permission control.** Choose per conversation whether the agent asks before every action, runs reads on its own, or runs everything.
- **Runs in the background.** Agents keep working when you switch chats or close Adeline. Open several windows and they all see the same live state.
- **Recovery.** Adeline retries failed turns and restores interrupted sessions when the harness supports it.
- **Plain files.** Agents and projects are YAML files under `~/.config/adeline/`. Edit them by hand and Adeline picks up the changes live.

## Supported agents

Adeline works with any harness that speaks the [Agent Client Protocol](https://agentclientprotocol.com). These have been tested:

- [oh-my-pi](https://omp.sh)
- [Claude Code](https://github.com/anthropics/claude-code) through the [claude-agent-acp](https://github.com/agentclientprotocol/claude-agent-acp) adapter
- [Codex](https://github.com/openai/codex) through the [codex-acp](https://github.com/agentclientprotocol/codex-acp) adapter
- [OpenCode](https://opencode.ai)

Adeline runs harnesses that are already installed on your machine. It never downloads or installs them for you. Sign in to each harness on its own before using it in Adeline.

## Install

There are no prebuilt releases yet. Build Adeline from source.

### Requirements

- [Rust](https://rustup.rs). The repository pins the toolchain, so `rustup` installs the right version on first build.
- **Windows:** Visual Studio Build Tools with the "Desktop development with C++" workload and a Windows SDK.
- **macOS:** the full Xcode app. Run `sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer` once.
- **Linux:** a graphical session with a working Vulkan driver. On Ubuntu or Debian, `./scripts/linux` installs the build dependencies.

### Build and run

```bash
git clone https://github.com/aseeon/adeline.git
cd adeline
cargo run --release --locked
```

Windows is the main development platform. macOS and Linux build and pass tests in CI but get less hands-on testing.

### Try it without agents

Demo mode opens a sample workspace with simulated replies. It does not start agents or touch your saved data.

```bash
cargo run --release --locked -- --demo
```

## Getting started

1. Install an ACP harness, for example OpenCode or Codex through codex-acp, and sign in to it.
2. Open Adeline and choose **Agents → Add an Agent**. Pick the harness, model, and effort level.
3. Create a project and select its working directory.
4. Start a new chat and send your first message.

## Contributing

Bug reports and pull requests are welcome. Please [open an issue](https://github.com/aseeon/adeline/issues) before starting on a large change.

Before you open a pull request, run the same checks as CI:

```bash
cargo fmt --all -- --check
./scripts/clippy
cargo nextest run --locked --all-features
```

On Windows, use `scripts\clippy.ps1`. If you don't have nextest, `cargo test --locked` works too.

## Credits

Adeline is built on [GPUI](https://github.com/zed-industries/zed) from the Zed team, [GPUI Kit](https://crates.io/crates/gpui-kit), the [Agent Client Protocol](https://agentclientprotocol.com), and [Phosphor Icons](https://phosphoricons.com).

## License

Adeline is licensed under the [Apache License 2.0](LICENSE).
