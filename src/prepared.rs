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
    pub fn matches(&self, query: &str, filter: usize, completed: bool) -> bool {
        self.matches_filter(filter, completed) && (query.is_empty() || self.text.contains(query))
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

pub fn search_records<'a>(
    records: impl IntoIterator<Item = &'a SearchRecord>,
    query: &str,
    filter: usize,
    completed: bool,
) -> Vec<usize> {
    records
        .into_iter()
        .enumerate()
        .filter(|(_, r)| r.matches(query, filter, completed))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
                id: i.to_string().into(),
                text: "body".into(),
                completed: false,
                blocked: false,
                working: false,
                unread: true,
            })
            .collect();
        let snapshot = catalog.clone();
        let mut changed = catalog[100].clone();
        changed.unread = false;
        catalog.replace(100, changed);
        assert_eq!(search_records(snapshot.iter(), "body", 3, true).len(), 200);
        assert_eq!(search_records(catalog.iter(), "body", 3, true).len(), 199);
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
                id: "a".into(),
                text: "first body".into(),
                completed: false,
                blocked: true,
                working: false,
                unread: true,
            },
            SearchRecord {
                id: "b".into(),
                text: "second body".into(),
                completed: true,
                blocked: false,
                working: false,
                unread: false,
            },
        ];
        assert_eq!(search_records(&records, "body", 0, true), [0, 1]);
        assert_eq!(search_records(&records, "body", 0, false), [0]);
        assert_eq!(search_records(&records, "", 1, true), [0]);
        assert_eq!(search_records(&records, "", 3, true), [0]);
        let mut generation = Generation::default();
        let older = generation.next();
        let latest = generation.next();
        let mut visible = vec![];
        // Latest query completes first; an older worker finishes afterwards.
        for (ticket, result) in [
            (latest, search_records(&records, "second", 0, true)),
            (older, search_records(&records, "first", 0, true)),
        ] {
            if generation.accepts(ticket) {
                visible = result;
            }
        }
        assert_eq!(visible, [1]);
    }
}
