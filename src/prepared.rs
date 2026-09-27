//! Chat search preparation is independent of rendering and safe on a worker thread.
use std::sync::Arc;

/// Keep the common prefix/suffix of ordered rows, including their measurements.
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

/// Each request owns a ticket. Cancelled or superseded results cannot publish.
#[derive(Default)]
pub struct Generation(u64);
impl Generation {
    pub fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
    pub fn accepts(&self, ticket: u64) -> bool {
        self.0 == ticket
    }
}

#[derive(Clone)]
pub struct SearchRecord {
    pub id: Arc<str>,
    pub text: SearchText,
    pub completed: bool,
    pub blocked: bool,
    pub working: bool,
    pub unread: bool,
    /// The chat's agent, as stored on the thread.
    pub agent: Arc<str>,
    /// Last activity in seconds since the epoch; 0 when unknown.
    pub activity: i64,
}

/// Sections of the chat list, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Chats that need input, then chats that are running.
    Current,
    Today,
    LastWeek,
    Earlier,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::Today => "Today",
            Self::LastWeek => "Last 7 days",
            Self::Earlier => "Earlier",
        }
    }
}

/// What the chat list shows. `filter` is 0 for all chats, 1 for chats that need
/// input and 3 for unread chats.
#[derive(Clone, Default, PartialEq)]
pub struct Criteria {
    pub query: String,
    pub filter: usize,
    pub completed: bool,
    pub agent: Option<Arc<str>>,
}

#[derive(Clone, Default)]
pub struct Outcome {
    /// Non-empty sections with their record indexes, most recent first.
    pub groups: Vec<(Group, Vec<usize>)>,
    /// Chats each tab would show (all, needing input, unread) for this search and agent.
    pub scopes: [usize; 3],
    /// Chats each agent would show for this search and tab, ordered by agent.
    pub agents: Vec<(Arc<str>, usize)>,
}

impl Outcome {
    pub fn matches(&self) -> usize {
        self.groups.iter().map(|(_, rows)| rows.len()).sum()
    }
}

/// One pass over the catalog: the visible sections plus the counts the tabs and
/// agent menu need, so the controls always describe what choosing them would show.
pub fn search<'a>(
    records: impl IntoIterator<Item = &'a SearchRecord>,
    criteria: &Criteria,
    now: i64,
) -> Outcome {
    let mut outcome = Outcome::default();
    let mut needing_input = Vec::new();
    let mut running = Vec::new();
    let mut periods: [Vec<(i64, usize)>; 3] = Default::default();
    for (index, record) in records.into_iter().enumerate() {
        if (!criteria.completed && record.completed)
            || !(criteria.query.is_empty() || record.text.contains(&criteria.query))
        {
            continue;
        }
        let agent_matches = criteria
            .agent
            .as_ref()
            .is_none_or(|agent| *agent == record.agent);
        if agent_matches {
            outcome.scopes[0] += 1;
            outcome.scopes[1] += usize::from(record.blocked);
            outcome.scopes[2] += usize::from(record.unread);
        }
        if !record.matches_filter(criteria.filter, criteria.completed) {
            continue;
        }
        match outcome
            .agents
            .binary_search_by(|(agent, _)| agent.as_ref().cmp(&record.agent))
        {
            Ok(slot) => outcome.agents[slot].1 += 1,
            Err(slot) => outcome.agents.insert(slot, (record.agent.clone(), 1)),
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
        if record.blocked {
            needing_input.push((activity, index));
        } else if record.working {
            running.push((activity, index));
        } else {
            let slot = match crate::recency::period(activity, now) {
                crate::recency::Period::Today => 0,
                crate::recency::Period::LastWeek => 1,
                crate::recency::Period::Earlier => 2,
            };
            periods[slot].push((activity, index));
        }
    }
    let newest_first = |rows: &mut Vec<(i64, usize)>| {
        rows.sort_by_key(|&(activity, index)| (std::cmp::Reverse(activity), index));
    };
    newest_first(&mut needing_input);
    newest_first(&mut running);
    let indexes = |rows: Vec<(i64, usize)>| rows.into_iter().map(|(_, index)| index);
    let mut groups = vec![(
        Group::Current,
        indexes(needing_input).chain(indexes(running)).collect(),
    )];
    for (group, mut rows) in [Group::Today, Group::LastWeek, Group::Earlier]
        .into_iter()
        .zip(periods)
    {
        newest_first(&mut rows);
        groups.push((group, indexes(rows).collect()));
    }
    outcome.groups = groups
        .into_iter()
        .filter(|(_, rows): &(Group, Vec<usize>)| !rows.is_empty())
        .collect();
    outcome
}

impl SearchRecord {
    pub fn matches_filter(&self, filter: usize, completed: bool) -> bool {
        (completed || !self.completed)
            && match filter {
                1 => self.blocked,
                2 => self.working,
                3 => self.unread,
                _ => true,
            }
    }
}

/// Worker snapshots share chunks. Updating one thread copies at most 64 records,
/// even while an older search is reading the previous snapshot.
#[derive(Clone, Debug)]
pub struct SharedSequence<T> {
    chunks: Vec<Arc<Vec<T>>>,
    len: usize,
}
impl<T> Default for SharedSequence<T> {
    fn default() -> Self {
        Self {
            chunks: Vec::new(),
            len: 0,
        }
    }
}
impl<T: Clone> SharedSequence<T> {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn get(&self, index: usize) -> Option<&T> {
        self.chunks.get(index / 64).and_then(|c| c.get(index % 64))
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.chunks.iter().flat_map(|c| c.iter())
    }
    pub fn replace(&mut self, index: usize, record: T) {
        Arc::make_mut(&mut self.chunks[index / 64])[index % 64] = record;
    }
    pub fn push(&mut self, item: T) {
        if self.len.is_multiple_of(64) {
            self.chunks.push(Arc::new(Vec::with_capacity(64)));
        }
        Arc::make_mut(self.chunks.last_mut().unwrap()).push(item);
        self.len += 1;
    }
}
impl<T> std::ops::Index<usize> for SharedSequence<T> {
    type Output = T;
    fn index(&self, index: usize) -> &Self::Output {
        &self.chunks[index / 64][index % 64]
    }
}
impl<T: Clone> FromIterator<T> for SharedSequence<T> {
    fn from_iter<I: IntoIterator<Item = T>>(records: I) -> Self {
        let mut result = Self::default();
        for record in records {
            result.push(record);
        }
        result
    }
}
pub type SearchCatalog = SharedSequence<SearchRecord>;

/// Immutable normalized chunks reuse the existing large prefix on append.
#[derive(Clone, Debug, Default)]
pub struct SearchText {
    chunks: SharedSequence<Arc<str>>,
    bytes: usize,
}
impl From<String> for SearchText {
    fn from(text: String) -> Self {
        Self {
            bytes: text.len(),
            chunks: [Arc::from(text)].into_iter().collect(),
        }
    }
}
impl From<&str> for SearchText {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}
impl SearchText {
    pub fn same_snapshot(&self, other: &Self) -> bool {
        self.bytes == other.bytes
            && self.chunks.len() == other.chunks.len()
            && self
                .chunks
                .chunks
                .iter()
                .zip(&other.chunks.chunks)
                .all(|(a, b)| Arc::ptr_eq(a, b))
    }
    pub fn len(&self) -> usize {
        self.bytes
    }
    pub fn append(&mut self, normalized: &str) {
        if normalized.is_empty() {
            return;
        }
        self.bytes += normalized.len();
        if let Some(last) = self.chunks.get(self.chunks.len().saturating_sub(1))
            && last.len() + normalized.len() <= 16 * 1024
        {
            let joined = format!("{last}{normalized}");
            self.chunks.replace(self.chunks.len() - 1, joined.into());
        } else {
            self.chunks.push(normalized.into());
        }
    }
    pub fn contains(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        if query.len() > self.bytes {
            return false;
        }
        if self.chunks.len() == 1 {
            return self.chunks[0].contains(query);
        }
        let mut tail = String::new();
        for chunk in self.chunks.iter() {
            if chunk.contains(query) {
                return true;
            }
            let mut end = chunk.len().min(query.len());
            while !chunk.is_char_boundary(end) {
                end += 1;
            }
            if !tail.is_empty() {
                tail.push_str(&chunk[..end]);
                if tail.contains(query) {
                    return true;
                }
            }
            if chunk.len() >= query.len() {
                let mut start = chunk.len() - query.len();
                while !chunk.is_char_boundary(start) {
                    start += 1;
                }
                tail.clear();
                tail.push_str(&chunk[start..]);
            } else if tail.is_empty() {
                tail.push_str(chunk);
            }
            if tail.len() > query.len() {
                let mut start = tail.len() - query.len();
                while !tail.is_char_boundary(start) {
                    start += 1;
                }
                tail.drain(..start);
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, text: &str) -> SearchRecord {
        SearchRecord {
            id: id.into(),
            text: text.into(),
            completed: false,
            blocked: false,
            working: false,
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

    fn criteria(query: &str, filter: usize, completed: bool) -> Criteria {
        Criteria {
            query: query.into(),
            filter,
            completed,
            agent: None,
        }
    }
    #[test]
    fn chunk_search_matches_contiguous_text_across_unicode_boundaries() {
        let chunks = [
            "prefix Ż",
            "ółć İ",
            "stanbul ",
            "body",
            " ",
            "second",
            " ΟΣ",
            " end",
        ];
        let flat = chunks.join("");
        let text = SearchText {
            chunks: chunks.iter().map(|s| Arc::from(*s)).collect(),
            bytes: flat.len(),
        };
        let boundaries: Vec<_> = flat
            .char_indices()
            .map(|(i, _)| i)
            .chain([flat.len()])
            .collect();
        for &start in &boundaries {
            for &end in boundaries.iter().filter(|&&end| end >= start) {
                assert!(
                    text.contains(&flat[start..end]),
                    "missing {:?}",
                    &flat[start..end]
                );
                let absent = format!("{}!", &flat[start..end]);
                assert_eq!(text.contains(&absent), flat.contains(&absent));
            }
        }
        let mut updated = text.clone();
        updated.append(" new content");
        assert!(updated.contains("end new content"));
        assert!(!text.contains("new content"));
    }
    #[test]
    fn catalog_updates_do_not_mutate_in_flight_search_snapshots() {
        let mut catalog: SearchCatalog = (0..200)
            .map(|i| SearchRecord {
                unread: true,
                ..record(&i.to_string(), "body")
            })
            .collect();
        let snapshot = catalog.clone();
        let mut changed = catalog[100].clone();
        changed.unread = false;
        catalog.replace(100, changed);
        let unread = criteria("body", 3, true);
        assert_eq!(search(snapshot.iter(), &unread, 0).matches(), 200);
        assert_eq!(search(catalog.iter(), &unread, 0).matches(), 199);
        assert!(Arc::ptr_eq(&snapshot.chunks[0], &catalog.chunks[0]));
        assert!(!Arc::ptr_eq(&snapshot.chunks[1], &catalog.chunks[1]));
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
        let first = generation.next();
        let second = generation.next();
        assert!(!generation.accepts(first));
        assert!(generation.accepts(second));
    }
    #[test]
    fn search_results_keep_status_filters_and_reject_out_of_order_completion() {
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
        let find = |query, filter, completed| {
            visible(&search(&records, &criteria(query, filter, completed), 0))
        };
        assert_eq!(find("body", 0, true), [0, 1]);
        assert_eq!(find("body", 0, false), [0]);
        assert_eq!(find("", 1, true), [0]);
        assert_eq!(find("", 3, true), [0]);
        let mut generation = Generation::default();
        let older = generation.next();
        let latest = generation.next();
        let mut shown = vec![];
        // Latest query completes first; an older worker finishes afterwards.
        for (ticket, result) in [
            (latest, find("second", 0, true)),
            (older, find("first", 0, true)),
        ] {
            if generation.accepts(ticket) {
                shown = result;
            }
        }
        assert_eq!(shown, [1]);
    }

    #[test]
    fn sections_put_live_chats_first_then_newest_by_period() {
        const NOW: i64 = 1_790_510_400;
        let day = 86_400;
        let records = [
            SearchRecord {
                activity: NOW - 20 * day,
                ..record("old", "")
            },
            SearchRecord {
                working: true,
                activity: NOW - 60,
                ..record("running", "")
            },
            SearchRecord {
                activity: NOW - 3 * day,
                ..record("week", "")
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
        ];
        let outcome = search(&records, &criteria("", 0, true), NOW);
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
                (Group::Current, vec!["waiting", "running"]),
                (Group::Today, vec!["today-newer", "today-older"]),
                (Group::LastWeek, vec!["week"]),
                (Group::Earlier, vec!["old"]),
            ]
        );
    }

    #[test]
    fn counts_describe_what_each_tab_and_agent_would_show() {
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
        let mut wanted = criteria("fix", 3, true);
        let outcome = search(&records, &wanted, 0);
        // Tabs ignore the tab itself; the agent menu ignores the agent itself.
        assert_eq!(outcome.scopes, [3, 1, 2]);
        assert_eq!(outcome.agents, [("claude".into(), 1), ("codex".into(), 1)]);
        assert_eq!(visible(&outcome), [0, 1]);
        wanted.agent = Some("claude".into());
        let outcome = search(&records, &wanted, 0);
        assert_eq!(outcome.scopes, [1, 0, 1]);
        assert_eq!(outcome.agents.len(), 2);
        assert_eq!(visible(&outcome), [1]);
        wanted.completed = false;
        assert_eq!(search(&records, &wanted, 0).scopes, [1, 0, 1]);
    }
}
