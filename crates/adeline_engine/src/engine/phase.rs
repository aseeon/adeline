//! One conversation's lifecycle, the engine's only record of it: whether its
//! agent worker runs or shuts down, whether a turn runs and why it stops, and
//! what the last turn left behind. `Live`'s phase flags (`project`) and the
//! runtime `ThreadStatus` (`status`) are computed from it. The methods here are
//! its transitions, called by the engine's handlers in `turns.rs`, `events.rs`,
//! `conversations.rs` and `projects.rs`.
//!
//! Storage failure is a flag beside the phase, since it can strike in any phase
//! and lasts until Retry storage. A turn that storage stops carries
//! `Stop::Storage`, and shows as recovering once storage is retried, until that
//! turn ends. `text_copy` marks a fork whose copied history no turn has
//! confirmed yet. A stopped turn leaves it set, since a stop can't tell whether
//! the history reached the agent.
use crate::acp;
use adeline_core::{
    conversation::{Attachment, ThreadStatus},
    protocol::{Live, PendingPermission},
};

#[derive(Default)]
pub(super) struct Lifecycle {
    pub phase: Phase,
    pub storage_failed: bool,
    pub text_copy: bool,
}

pub(super) enum Phase {
    Idle { agent: Option<Agent>, after: After },
    Turn { agent: Agent, turn: Turn },
}

pub(super) struct Agent {
    pub driver: acp::Driver,
    pub turn: u64,
    pub steering: Option<(String, Vec<Attachment>)>,
    pub closing: Option<Closing>,
}

#[derive(Default)]
pub(super) struct Closing {
    pub stuck: bool,
    pub restart: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum After {
    #[default]
    Done,
    Blocked,
    LoginRequired,
    Replacement,
}

#[derive(Default)]
pub(super) struct Turn {
    pub permission: Vec<PendingPermission>,
    pub retry: Option<Retry>,
    pub stop: Option<Stop>,
}

pub(super) struct Retry {
    pub attempt: u32,
    pub limit: u32,
    pub error: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Stop {
    Shutdown,
    User,
    Permission,
    SendQueued,
    Storage,
}

impl Default for Phase {
    fn default() -> Self {
        Self::Idle {
            agent: None,
            after: After::Done,
        }
    }
}

impl Phase {
    pub(super) fn agent(&self) -> Option<&Agent> {
        match self {
            Self::Idle { agent, .. } => agent.as_ref(),
            Self::Turn { agent, .. } => Some(agent),
        }
    }

    pub(super) fn agent_mut(&mut self) -> Option<&mut Agent> {
        match self {
            Self::Idle { agent, .. } => agent.as_mut(),
            Self::Turn { agent, .. } => Some(agent),
        }
    }

    pub(super) fn turn(&self) -> Option<&Turn> {
        match self {
            Self::Turn { turn, .. } => Some(turn),
            Self::Idle { .. } => None,
        }
    }

    pub(super) fn turn_mut(&mut self) -> Option<&mut Turn> {
        match self {
            Self::Turn { turn, .. } => Some(turn),
            Self::Idle { .. } => None,
        }
    }
}

impl Lifecycle {
    pub(super) fn loaded(status: ThreadStatus, storage_failed: bool) -> Self {
        Self {
            phase: Phase::Idle {
                agent: None,
                after: if status == ThreadStatus::Blocked {
                    After::Blocked
                } else {
                    After::Done
                },
            },
            storage_failed,
            text_copy: false,
        }
    }

    pub(super) fn after(&self) -> Option<After> {
        match &self.phase {
            Phase::Idle { after, .. } => Some(*after),
            Phase::Turn { .. } => None,
        }
    }

    pub(super) fn closing(&self) -> Option<&Closing> {
        self.phase.agent()?.closing.as_ref()
    }

    fn restarting(&self) -> bool {
        self.closing()
            .is_some_and(|closing| closing.restart.is_some())
    }

    pub(super) fn processing(&self) -> bool {
        self.phase.turn().is_some() || self.restarting()
    }

    pub(super) fn recovering(&self) -> bool {
        !self.storage_failed
            && self
                .phase
                .turn()
                .is_some_and(|turn| turn.stop == Some(Stop::Storage))
    }

    pub(super) fn spawned(&mut self, new: Agent) {
        if let Phase::Idle { agent, .. } = &mut self.phase {
            agent.get_or_insert(new);
        }
    }

    pub(super) fn start(&mut self) -> bool {
        match std::mem::take(&mut self.phase) {
            Phase::Idle {
                agent: Some(agent), ..
            } if agent.closing.is_none() => {
                self.phase = Phase::Turn {
                    agent,
                    turn: Turn::default(),
                };
                true
            }
            phase => {
                self.phase = phase;
                false
            }
        }
    }

    pub(super) fn end(&mut self) -> Option<Turn> {
        match std::mem::take(&mut self.phase) {
            Phase::Turn { agent, turn } => {
                self.phase = Phase::Idle {
                    agent: Some(agent),
                    after: After::Done,
                };
                Some(turn)
            }
            phase @ Phase::Idle { .. } => {
                self.phase = phase;
                None
            }
        }
    }

    pub(super) fn block(&mut self, outcome: After) {
        if let Phase::Idle { after, .. } = &mut self.phase
            && *after != After::Replacement
        {
            *after = outcome;
        }
    }

    pub(super) fn stop(&mut self, reason: Stop) -> Option<&acp::Driver> {
        let Phase::Turn { agent, turn } = &mut self.phase else {
            return None;
        };
        turn.stop = turn.stop.max(Some(reason));
        turn.permission.clear();
        Some(&agent.driver)
    }

    pub(super) fn close(&mut self) -> Option<&acp::Driver> {
        self.stop(Stop::Shutdown);
        let agent = self.phase.agent_mut()?;
        if agent.closing.is_some() {
            return None;
        }
        agent.closing = Some(Closing::default());
        Some(&agent.driver)
    }

    pub(super) fn force_stop(&mut self) -> Option<&acp::Driver> {
        self.stop(Stop::User);
        let agent = self.phase.agent_mut()?;
        agent.closing.get_or_insert_default();
        Some(&agent.driver)
    }

    pub(super) fn restart(&mut self, prompt: String) -> Option<&acp::Driver> {
        let agent = self.phase.agent_mut()?;
        agent.closing.get_or_insert_default().restart = Some(prompt);
        Some(&agent.driver)
    }

    pub(super) fn gone(&mut self) -> Option<(Agent, Option<Turn>)> {
        let (agent, turn, after) = match std::mem::take(&mut self.phase) {
            Phase::Idle { agent, after } => (agent, None, after),
            Phase::Turn { agent, turn } => (Some(agent), Some(turn), After::Done),
        };
        self.phase = Phase::Idle { agent: None, after };
        agent.map(|agent| (agent, turn))
    }

    pub(super) fn project(&self, live: &mut Live) {
        let agent = self.phase.agent();
        let closing = self.closing();
        let turn = self.phase.turn();
        live.running = agent.is_some();
        live.processing = self.processing();
        live.shutting_down = closing.is_some();
        live.shutdown_stuck = closing.is_some_and(|closing| closing.stuck);
        live.storage_failed = self.storage_failed;
        live.recovering_storage = self.recovering();
        live.replacement = self.after() == Some(After::Replacement);
        live.auth_required = self.after() == Some(After::LoginRequired);
        live.permission = turn.map(|turn| turn.permission.clone()).unwrap_or_default();
        live.progress = if self.restarting() {
            Some("Restarting the agent…".into())
        } else if closing.is_some() {
            None
        } else {
            turn.and_then(Turn::progress)
        };
        if !live.processing {
            live.quiet_since = None;
        }
    }

    pub(super) fn status(&self, saved: ThreadStatus) -> ThreadStatus {
        let turn_runs = matches!(&self.phase, Phase::Turn { agent, .. } if agent.closing.is_none());
        if saved.closed() && (self.storage_failed || !turn_runs) {
            return saved;
        }
        if self.storage_failed {
            return ThreadStatus::Blocked;
        }
        match &self.phase {
            Phase::Turn { turn, .. } if !turn.permission.is_empty() => ThreadStatus::Blocked,
            _ if self.processing() => ThreadStatus::Processing,
            Phase::Idle {
                after: After::Done, ..
            } => ThreadStatus::Idle,
            _ => ThreadStatus::Blocked,
        }
    }

    pub(super) fn state(&self) -> Option<&'static str> {
        if !self.processing() {
            return None;
        }
        Some(match self.phase.turn() {
            Some(turn) if !turn.permission.is_empty() => "waiting for permission",
            Some(turn) if turn.retry.is_some() => "retrying",
            _ => "processing",
        })
    }
}

impl Turn {
    fn progress(&self) -> Option<String> {
        if self.stop.is_some() {
            return Some("Stopping…".into());
        }
        self.retry.as_ref().map(|retry| {
            format!(
                "Retry {} of {}: {}",
                retry.attempt, retry.limit, retry.error
            )
        })
    }
}
