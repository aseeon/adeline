//! Doc text as read from disk and as saved: a file that is too large, binary
//! or not UTF-8 opens read-only, and a save keeps the file's line endings and
//! its trailing newline, or lack of one.

// Rust guideline compliant 2026-10-09

/// Larger files open read-only.
pub const MAX_EDITABLE: usize = 10 * 1024 * 1024;

// Git's heuristic: a NUL byte near the start means binary.
const BINARY_PROBE: usize = 8000;

/// The text of a file, and why it's read-only, if it is.
pub fn decode(bytes: Vec<u8>) -> (String, Option<&'static str>) {
    let probe = &bytes[..bytes.len().min(BINARY_PROBE)];
    if probe.contains(&0) {
        return (
            String::from_utf8_lossy(&bytes).into_owned(),
            Some("This file looks binary, so it opens read-only."),
        );
    }
    let large = bytes.len() > MAX_EDITABLE;
    match String::from_utf8(bytes) {
        Ok(text) => (
            text,
            large.then_some("This file is larger than 10 MB, so it opens read-only."),
        ),
        Err(error) => (
            String::from_utf8_lossy(error.as_bytes()).into_owned(),
            Some("This file is not UTF-8 text, so it opens read-only."),
        ),
    }
}

/// `text` with the line endings and trailing newline of `original`, the file's text on disk.
pub fn conform(text: &str, original: &str) -> String {
    let crlf = original
        .find('\n')
        .is_some_and(|at| original[..at].ends_with('\r'));
    let mut body = text.replace("\r\n", "\n");
    if !original.is_empty() {
        if body.ends_with('\n') {
            body.pop();
        }
        if original.ends_with('\n') {
            body.push('\n');
        }
    }
    if crlf {
        body = body.replace('\n', "\r\n");
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_keeps_line_endings_and_the_final_newline_as_they_were() {
        // An editor shows LF lines, whatever the file holds.
        let original = "one\r\ntwo\r\nthree";
        let saved = conform("one\nTWO\nthree", original);
        assert_eq!(saved, "one\r\nTWO\r\nthree");
        let changed: Vec<_> = original
            .split("\r\n")
            .zip(saved.split("\r\n"))
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(changed, [("two", "TWO")]);
        assert_eq!(conform("a\nb", "a\nb\n"), "a\nb\n");
        assert_eq!(conform("a\nb\n", "a\nb"), "a\nb");
        assert_eq!(conform("a\n\n\n", "a\n"), "a\n\n\n");
        assert_eq!(conform("new\r\n", ""), "new\n");
    }

    #[test]
    fn unreadable_text_opens_read_only_with_a_reason() {
        assert_eq!(decode(b"plain".to_vec()), ("plain".into(), None));
        let (text, reason) = decode(b"abc\xff".to_vec());
        assert_eq!(text, "abc\u{fffd}");
        assert!(reason.is_some_and(|r| r.contains("UTF-8")));
        assert!(
            decode(b"a\0b".to_vec())
                .1
                .is_some_and(|r| r.contains("binary"))
        );
        let mut large = vec![b'a'; MAX_EDITABLE];
        assert_eq!(decode(large.clone()).1, None);
        large.push(b'a');
        assert!(decode(large).1.is_some_and(|r| r.contains("10 MB")));
    }
}
