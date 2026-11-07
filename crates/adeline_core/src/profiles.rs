//! Agent profiles: everything Adeline knows about one supported agent beyond
//! generic ACP. Agents without a profile, including Custom agents
//! and other registry agents, get generic behavior.

pub const CUSTOM: &str = "custom";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instructions {
    None,
    Flags,
    SessionMeta,
}

#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub program: &'static str,
    pub arguments: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
pub struct Install {
    pub windows: &'static [Step],
    pub unix: &'static [Step],
    pub needs_node: bool,
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
    pub harness: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub arguments: &'static [&'static str],
    pub install: Install,
    pub instructions: Instructions,
    pub plan_modes: &'static [&'static str],
    pub mode_notes: &'static [(&'static str, &'static str)],
    pub turn_end: &'static [&'static str],
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
    steering: false,
};

pub const PI: Profile = Profile {
    harness: "pi-acp",
    name: "Pi",
    command: "pi-acp",
    arguments: &[],
    install: Install {
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
        windows: &[npm(&["install", "-g", "@opencode/cli"])],
        unix: &[npm(&["install", "-g", "@opencode/cli"])],
        needs_node: true,
        package: Some("@opencode/cli"),
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

pub fn find(harness: &str, identity: &str) -> Option<&'static Profile> {
    let identity = identity.to_lowercase();
    ALL.iter().find(|profile| {
        profile.harness == harness
            || (harness == CUSTOM
                && !identity.is_empty()
                && (identity == profile.command
                    || identity == profile.harness
                    || (profile.harness == "omp" && identity == "oh-my-pi")
                    || (profile.harness == "claude-acp"
                        && identity == "@agentclientprotocol/claude-agent-acp")))
    })
}

pub fn instructions(harness: &str, identity: &str) -> Instructions {
    find(harness, identity).map_or(Instructions::None, |profile| profile.instructions)
}

pub fn is_plan_mode(profile: Option<&Profile>, mode: &str) -> bool {
    profile.is_some_and(|profile| profile.plan_modes.contains(&mode))
}

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
        let harness = |harness, identity| find(harness, identity).map(|profile| profile.harness);
        assert_eq!(harness("claude-acp", ""), Some("claude-acp"));
        assert_eq!(harness(CUSTOM, "oh-my-pi"), Some("omp"));
        assert_eq!(
            harness(CUSTOM, "@agentclientprotocol/claude-agent-acp"),
            Some("claude-acp")
        );
        assert_eq!(harness(CUSTOM, "Codex-ACP"), Some("codex-acp"));
        assert_eq!(harness(CUSTOM, ""), None);
        assert_eq!(harness("gemini", "gemini-cli"), None);
    }
}
