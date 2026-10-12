//! Chat search preparation, independent of rendering and safe on a worker thread.
//!
//! `Group` is in display order and each group lists its chats newest first. The
//! agent counts ignore the agent filter itself. Only the latest `Generation` ticket
//! may publish a result.
use std::sync::Arc;

pub fn changed_range<T: PartialEq>(
    old: &[T],
    new: &[T],
) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (prefix..old.len() - suffix, prefix..new.len() - suffix)
}

#[derive(Debug, Default)]
pub struct Generation(u64);
impl Generation {
    pub fn issue(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
    pub fn accepts(&self, ticket: u64) -> bool {
        self.0 == ticket
    }
}

#[derive(Debug, Clone)]
pub struct SearchRecord {
    pub id: Arc<str>,
    pub text: Arc<str>,
    pub completed: bool,
    pub archived: bool,
    pub blocked: bool,
    pub processing: bool,
    pub unread: bool,
    pub agent: Arc<str>,
    // Seconds since the epoch, 0 when unknown.
    pub activity: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    NeedsInput,
    Processing,
    Today,
    LastThreeDays,
    Earlier,
    Completed,
    Archived,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Self::NeedsInput => "Needs Input",
            Self::Processing => "Processing",
            Self::Today => "Today",
            Self::LastThreeDays => "Last 3 days",
            Self::Earlier => "Earlier",
            Self::Completed => "Completed",
            Self::Archived => "Archived",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Criteria {
    pub query: String,
    pub completed: bool,
    pub archived: bool,
    pub agent: Option<Arc<str>>,
}

impl Criteria {
    fn hides(&self, record: &SearchRecord) -> bool {
        (record.completed && !self.completed) || (record.archived && !self.archived)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub groups: Vec<(Group, Vec<usize>)>,
    pub agents: Vec<(Arc<str>, usize)>,
}

impl Outcome {
    pub fn matches(&self) -> usize {
        self.groups.iter().map(|(_, rows)| rows.len()).sum()
    }
}

pub fn search<'a>(
    records: impl IntoIterator<Item = &'a SearchRecord>,
    criteria: &Criteria,
    now: i64,
) -> Outcome {
    let mut outcome = Outcome::default();
    let mut needing_input = Vec::new();
    let mut running = Vec::new();
    let mut periods: [Vec<(i64, usize)>; 3] = Default::default();
    let mut completed = Vec::new();
    let mut archived = Vec::new();
    for (index, record) in records.into_iter().enumerate() {
        if criteria.hides(record)
            || !(criteria.query.is_empty() || record.text.contains(&criteria.query))
        {
            continue;
        }
        let agent_matches = criteria
            .agent
            .as_ref()
            .is_none_or(|agent| *agent == record.agent);
        match outcome
            .agents
            .binary_search_by(|(agent, _)| agent.as_ref().cmp(&record.agent))
        {
            Ok(slot) => outcome.agents[slot].1 += 1,
            Err(slot) => outcome.agents.insert(slot, (Arc::clone(&record.agent), 1)),
        }
        if !agent_matches {
            continue;
        }
        // A chat without a readable time was just created on this machine.
        let activity = if record.activity == 0 {
            now
        } else {
            record.activity
        };
        if record.archived {
            archived.push((activity, index));
        } else if record.completed {
            completed.push((activity, index));
        } else if record.blocked {
            needing_input.push((activity, index));
        } else if record.processing {
            running.push((activity, index));
        } else {
            let slot = match crate::recency::period(activity, now) {
                crate::recency::Period::Today => 0,
                crate::recency::Period::LastThreeDays => 1,
                crate::recency::Period::Earlier => 2,
            };
            periods[slot].push((activity, index));
        }
    }
    let newest_first = |rows: &mut Vec<(i64, usize)>| {
        rows.sort_by_key(|&(activity, index)| (std::cmp::Reverse(activity), index));
    };
    let [today, recent, earlier] = periods;
    let mut groups = Vec::with_capacity(7);
    for (group, mut rows) in [
        (Group::NeedsInput, needing_input),
        (Group::Processing, running),
        (Group::Today, today),
        (Group::LastThreeDays, recent),
        (Group::Earlier, earlier),
        (Group::Completed, completed),
        (Group::Archived, archived),
    ] {
        newest_first(&mut rows);
        groups.push((group, rows.into_iter().map(|(_, index)| index).collect()));
    }
    outcome.groups = groups
        .into_iter()
        .filter(|(_, rows): &(Group, Vec<usize>)| !rows.is_empty())
        .collect();
    outcome
}

pub fn rail(
    records: &[SearchRecord],
    completed: bool,
    archived: bool,
    selected: Option<usize>,
    now: i64,
) -> Vec<usize> {
    let criteria = Criteria {
        completed,
        archived,
        ..Criteria::default()
    };
    search(records.iter(), &criteria, now)
        .groups
        .into_iter()
        .flat_map(|(_, rows)| rows)
        // The list pins the open chat above the rail.
        .filter(|&row| Some(row) != selected)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, text: &str) -> SearchRecord {
        SearchRecord {
            id: id.into(),
            text: text.into(),
            completed: false,
            archived: false,
            blocked: false,
            processing: false,
            unread: false,
            agent: "codex".into(),
            activity: 0,
        }
    }

    fn visible(outcome: &Outcome) -> Vec<usize> {
        outcome
            .groups
            .iter()
            .flat_map(|(_, rows)| rows.iter().copied())
            .collect()
    }

    fn criteria(query: &str, completed: bool) -> Criteria {
        Criteria {
            query: query.into(),
            completed,
            archived: false,
            agent: None,
        }
    }
    #[test]
    fn row_changes_keep_the_common_prefix_and_suffix() {
        for (old, new, expected) in [
            (vec![1, 2, 3], vec![1, 2, 3, 4], (3..3, 3..4)),
            (vec![1, 2, 3], vec![1, 9, 3], (1..2, 1..2)),
            (vec![1, 2, 3], vec![1, 3], (1..2, 1..1)),
            (vec![1, 2, 3], vec![1, 2, 3], (3..3, 3..3)),
            (vec![], vec![1], (0..0, 0..1)),
        ] {
            assert_eq!(changed_range(&old, &new), expected);
        }
    }
    #[test]
    fn only_latest_request_can_publish() {
        let mut generation = Generation::default();
        let first = generation.issue();
        let second = generation.issue();
        assert!(!generation.accepts(first));
        assert!(generation.accepts(second));
    }
    #[test]
    fn search_results_reject_out_of_order_completion() {
        let records = [
            SearchRecord {
                blocked: true,
                unread: true,
                ..record("a", "first body")
            },
            SearchRecord {
                completed: true,
                ..record("b", "second body")
            },
        ];
        let find = |query, completed| visible(&search(&records, &criteria(query, completed), 0));
        assert_eq!(find("body", true), [0, 1]);
        assert_eq!(find("body", false), [0]);
        let mut generation = Generation::default();
        let older = generation.issue();
        let latest = generation.issue();
        let mut shown = vec![];
        // Latest query completes first; an older worker finishes afterwards.
        for (ticket, result) in [(latest, find("second", true)), (older, find("first", true))] {
            if generation.accepts(ticket) {
                shown = result;
            }
        }
        assert_eq!(shown, [1]);
    }

    #[test]
    fn sections_put_live_chats_first_then_newest_by_period_then_finished() {
        const NOW: i64 = 1_790_510_400;
        let day = 86_400;
        let records = [
            SearchRecord {
                activity: NOW - 20 * day,
                ..record("old", "")
            },
            SearchRecord {
                processing: true,
                activity: NOW - 60,
                ..record("running", "")
            },
            SearchRecord {
                activity: NOW - 2 * day,
                ..record("recent", "")
            },
            SearchRecord {
                activity: NOW - 600,
                ..record("today-older", "")
            },
            SearchRecord {
                blocked: true,
                activity: NOW - 9 * day,
                ..record("waiting", "")
            },
            SearchRecord {
                activity: NOW - 60,
                ..record("today-newer", "")
            },
            SearchRecord {
                completed: true,
                activity: NOW - 30,
                ..record("done", "")
            },
            SearchRecord {
                archived: true,
                activity: NOW - 20,
                ..record("put-away", "")
            },
        ];
        let everything = Criteria {
            archived: true,
            ..criteria("", true)
        };
        let outcome = search(&records, &everything, NOW);
        let sections: Vec<_> = outcome
            .groups
            .iter()
            .map(|(group, rows)| {
                (
                    *group,
                    rows.iter().map(|&i| &*records[i].id).collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            sections,
            [
                (Group::NeedsInput, vec!["waiting"]),
                (Group::Processing, vec!["running"]),
                (Group::Today, vec!["today-newer", "today-older"]),
                (Group::LastThreeDays, vec!["recent"]),
                (Group::Earlier, vec!["old"]),
                (Group::Completed, vec!["done"]),
                (Group::Archived, vec!["put-away"]),
            ]
        );
        // Each finished section can be hidden on its own; archived is by default.
        let shown = |criteria: &Criteria| -> Vec<Group> {
            search(&records, criteria, NOW)
                .groups
                .iter()
                .map(|(group, _)| *group)
                .collect()
        };
        let default = shown(&criteria("", true));
        assert!(default.contains(&Group::Completed) && !default.contains(&Group::Archived));
        let archived_only = shown(&Criteria {
            archived: true,
            ..criteria("", false)
        });
        assert!(!archived_only.contains(&Group::Completed));
        assert!(archived_only.contains(&Group::Archived));
    }

    #[test]
    fn rail_lists_every_chat_in_list_order_without_the_open_one() {
        const NOW: i64 = 1_790_510_400;
        let day = 86_400;
        let catalog: Vec<SearchRecord> = [
            SearchRecord {
                activity: NOW - 3 * day,
                ..record("older", "")
            },
            SearchRecord {
                activity: NOW - 60,
                ..record("today", "")
            },
            SearchRecord {
                blocked: true,
                activity: NOW - 9 * day,
                ..record("waiting", "")
            },
            SearchRecord {
                completed: true,
                activity: NOW - 120,
                ..record("done-today", "")
            },
        ]
        .into_iter()
        .collect();
        let ids = |rows: Vec<usize>| {
            rows.into_iter()
                .map(|i| catalog.get(i).unwrap().id.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(rail(&catalog, true, false, None, NOW)),
            ["waiting", "today", "older", "done-today"]
        );
        assert_eq!(
            ids(rail(&catalog, false, false, None, NOW)),
            ["waiting", "today", "older"]
        );
        // The open chat is pinned above the rail, so the rail leaves it out.
        assert_eq!(
            ids(rail(&catalog, false, false, Some(1), NOW)),
            ["waiting", "older"]
        );
        assert!(rail(&Vec::new(), true, true, None, NOW).is_empty());
    }

    #[test]
    fn counts_describe_what_each_agent_would_show() {
        let records = [
            SearchRecord {
                blocked: true,
                unread: true,
                ..record("a", "fix build")
            },
            SearchRecord {
                agent: "claude".into(),
                unread: true,
                ..record("b", "fix docs")
            },
            SearchRecord {
                agent: "claude".into(),
                ..record("c", "write notes")
            },
            SearchRecord {
                completed: true,
                ..record("d", "fix tests")
            },
        ];
        let mut wanted = criteria("fix", true);
        let outcome = search(&records, &wanted, 0);
        assert_eq!(outcome.agents, [("claude".into(), 1), ("codex".into(), 2)]);
        assert_eq!(visible(&outcome), [0, 1, 3]);
        // The agent menu ignores the agent itself.
        wanted.agent = Some("claude".into());
        let outcome = search(&records, &wanted, 0);
        assert_eq!(outcome.agents.len(), 2);
        assert_eq!(visible(&outcome), [1]);
    }
}
