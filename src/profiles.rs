//! Agent profiles: everything Adeline knows about one supported agent beyond
//! generic ACP (scope R6). Agents without a profile, including Custom agents
//! and other registry agents, get generic behavior.

/// How an agent receives Adeline's system instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instructions {
    /// No verified mechanism; the agent form hides the field.
    None,
    /// `--append-system-prompt=` or `--system-prompt=` on the command line (OMP).
    Flags,
    /// `_meta.systemPrompt` on session setup: a string replaces the default
    /// prompt, `{append}` adds to it (claude-agent-acp).
    SessionMeta,
}

/// One command of an install: a program and its arguments.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub program: &'static str,
    pub arguments: &'static [&'static str],
}

/// The documented, standard global installation of an agent on one OS.
#[derive(Clone, Copy, Debug)]
pub struct Install {
    pub windows: &'static [Step],
    pub unix: &'static [Step],
    /// The steps run `npm`, so Node.js must be installed first.
    pub needs_node: bool,
    /// The npm package whose installed version is compared with the registry.
    pub package: Option<&'static str>,
}

impl Install {
    pub fn steps(&self) -> &'static [Step] {
        if cfg!(windows) {
            self.windows
        } else {
            self.unix
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Profile {
    /// The registry ID, or `omp` for the built-in OMP entry.
    pub harness: &'static str,
    /// The display name used in sentences like "Claude Code needs Node.js."
    pub name: &'static str,
    /// The executable and its arguments.
    pub command: &'static str,
    pub arguments: &'static [&'static str],
    pub install: Install,
    pub instructions: Instructions,
    /// Mode IDs that are plan modes: hidden from every mode choice (scope R20).
    pub plan_modes: &'static [&'static str],
    /// Descriptions for modes whose own description is missing or unclear.
    pub mode_notes: &'static [(&'static str, &'static str)],
    /// `_meta` keys that, set to `true` on a session update, end the running
    /// turn without a prompt response (scope R33).
    pub turn_end: &'static [&'static str],
    /// Whether Send now may steer the running turn when the agent advertises
    /// steering. Off for adapters that start a detached turn instead.
    pub steering: bool,
}

const fn npm(package: &'static [&'static str]) -> Step {
    Step {
        program: "npm",
        arguments: package,
    }
}

pub const CLAUDE: Profile = Profile {
    harness: "claude-acp",
    name: "Claude Code",
    command: "claude-agent-acp",
    arguments: &[],
    install: Install {
        windows: &[npm(&[
            "install",
            "-g",
            "@agentclientprotocol/claude-agent-acp",
        ])],
        unix: &[npm(&[
            "install",
            "-g",
            "@agentclientprotocol/claude-agent-acp",
        ])],
        needs_node: true,
        package: Some("@agentclientprotocol/claude-agent-acp"),
    },
    instructions: Instructions::SessionMeta,
    plan_modes: &["plan"],
    mode_notes: &[
        ("default", "Asks before editing files or running commands."),
        (
            "acceptEdits",
            "Edits files without asking; asks before commands.",
        ),
        (
            "auto",
            "Decides for itself which actions need your approval.",
        ),
        (
            "dontAsk",
            "Never asks; anything not allowed in advance is denied.",
        ),
        ("bypassPermissions", "Runs every tool without asking."),
    ],
    turn_end: &[],
    steering: true,
};

pub const CODEX: Profile = Profile {
    harness: "codex-acp",
    name: "Codex",
    command: "codex-acp",
    arguments: &[],
    install: Install {
        windows: &[npm(&["install", "-g", "@agentclientprotocol/codex-acp"])],
        unix: &[npm(&["install", "-g", "@agentclientprotocol/codex-acp"])],
        needs_node: true,
        package: Some("@agentclientprotocol/codex-acp"),
    },
    instructions: Instructions::None,
    plan_modes: &["plan"],
    mode_notes: &[
        (
            "read-only",
            "Reads files; asks before editing or running commands.",
        ),
        (
            "auto",
            "Edits and runs commands in the project; asks for anything outside it.",
        ),
        (
            "full-access",
            "Edits, runs commands and uses the network without asking.",
        ),
    ],
    turn_end: &[],
    // codex-acp answers steering at a turn's end with a detached new turn.
    steering: false,
};

pub const PI: Profile = Profile {
    harness: "pi-acp",
    name: "Pi",
    command: "pi-acp",
    arguments: &[],
    install: Install {
        // pi-acp drives the `pi` CLI, which is installed in the same offer.
        windows: &[
            npm(&["install", "-g", "@earendil-works/pi-coding-agent"]),
            npm(&["install", "-g", "pi-acp"]),
        ],
        unix: &[
            npm(&["install", "-g", "@earendil-works/pi-coding-agent"]),
            npm(&["install", "-g", "pi-acp"]),
        ],
        needs_node: true,
        package: Some("pi-acp"),
    },
    instructions: Instructions::None,
    plan_modes: &[],
    mode_notes: &[],
    turn_end: &[],
    steering: true,
};

pub const OPENCODE: Profile = Profile {
    harness: "opencode",
    name: "OpenCode",
    command: "opencode",
    arguments: &["acp"],
    install: Install {
        windows: &[npm(&["install", "-g", "opencode-ai"])],
        unix: &[npm(&["install", "-g", "opencode-ai"])],
        needs_node: true,
        package: Some("opencode-ai"),
    },
    instructions: Instructions::None,
    plan_modes: &["plan"],
    mode_notes: &[(
        "build",
        "Edits files and runs commands to carry out the work.",
    )],
    turn_end: &[],
    steering: true,
};

pub const OMP: Profile = Profile {
    harness: "omp",
    name: "OMP",
    command: "omp",
    arguments: &["acp"],
    install: Install {
        windows: &[Step {
            program: "powershell",
            arguments: &[
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "irm https://omp.sh/install.ps1 | iex",
            ],
        }],
        unix: &[Step {
            program: "sh",
            arguments: &["-c", "curl -fsSL https://omp.sh/install | sh"],
        }],
        needs_node: false,
        package: None,
    },
    instructions: Instructions::Flags,
    plan_modes: &[],
    mode_notes: &[],
    turn_end: &[],
    steering: true,
};

pub const ALL: [Profile; 5] = [CLAUDE, CODEX, PI, OPENCODE, OMP];

/// The profile of a harness, by registry ID. A Custom agent matches by the
/// name it reports in its handshake, so a hand-configured OMP is still OMP.
pub fn find(harness: &str, identity: &str) -> Option<&'static Profile> {
    let identity = identity.to_lowercase();
    ALL.iter().find(|profile| {
        profile.harness == harness
            || (harness == crate::harness::CUSTOM
                && !identity.is_empty()
                && (identity == profile.command
                    || identity == profile.harness
                    || (profile.harness == "omp" && identity == "oh-my-pi")
                    || (profile.harness == "claude-acp"
                        && identity == "@agentclientprotocol/claude-agent-acp")))
    })
}

/// How a harness takes instructions; generic agents take none.
pub fn instructions(harness: &str, identity: &str) -> Instructions {
    find(harness, identity).map_or(Instructions::None, |profile| profile.instructions)
}

/// Whether a mode is one of the profile's hidden plan modes.
pub fn is_plan_mode(profile: Option<&Profile>, mode: &str) -> bool {
    profile.is_some_and(|profile| profile.plan_modes.contains(&mode))
}

/// The agent's mode description, else the profile's supplement.
pub fn mode_description(profile: Option<&Profile>, mode: &str, given: &str) -> String {
    if !given.trim().is_empty() {
        return given.to_owned();
    }
    profile
        .and_then(|profile| profile.mode_notes.iter().find(|(id, _)| *id == mode))
        .map(|(_, note)| (*note).to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_found_by_harness_or_reported_identity() {
        assert_eq!(find("claude-acp", "").unwrap().name, "Claude Code");
        assert_eq!(
            find(crate::harness::CUSTOM, "oh-my-pi").unwrap().harness,
            "omp"
        );
        assert!(find(crate::harness::CUSTOM, "").is_none());
        assert!(find("gemini", "gemini-cli").is_none());
        assert_eq!(instructions("omp", ""), Instructions::Flags);
        assert_eq!(instructions("codex-acp", ""), Instructions::None);
        assert!(is_plan_mode(Some(&CLAUDE), "plan"));
        assert!(!is_plan_mode(None, "plan"));
        assert_eq!(
            mode_description(Some(&CLAUDE), "default", ""),
            "Asks before editing files or running commands."
        );
        assert_eq!(
            mode_description(Some(&CLAUDE), "default", "Theirs"),
            "Theirs"
        );
        // Pi's offer installs the `pi` CLI too.
        assert!(
            PI.install.steps()[0]
                .arguments
                .contains(&"@earendil-works/pi-coding-agent")
        );
    }
}
