//! The Markdown toolbar of the doc editor: each tool turns the text and its
//! selection into one [`Edit`]. Inline styles wrap or unwrap the selection,
//! line tools set or clear a prefix on every line the selection touches, and
//! the block tools put a code fence or a table on lines of their own.
//!
//! Offsets are UTF-8 byte offsets. A selection past the end or off a char
//! boundary is clamped, so no input panics. `\r\n` counts as one line end.

// Rust guideline compliant 2026-10-09

use std::ops::Range;

/// A toolbar tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// A heading of this level, clamped to 1..=6.
    Heading(u8),
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    CodeBlock,
    Link,
    BulletList,
    NumberedList,
    TaskList,
    Quote,
    Table,
}

/// Replace `range` of the old text with `text`, then select `selection` of the new text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
    pub selection: Range<usize>,
}

/// The edit that applies `format` to `selection` of `text`.
pub fn apply(format: Format, text: &str, selection: Range<usize>) -> Edit {
    let selection = clamp(text, selection);
    match format {
        Format::Bold => inline(text, selection, "**"),
        Format::Italic => inline(text, selection, "*"),
        Format::Strikethrough => inline(text, selection, "~~"),
        Format::InlineCode => inline(text, selection, "`"),
        Format::Link => link(text, selection),
        Format::CodeBlock => code_block(text, selection),
        Format::Table => table(text, &selection),
        Format::Heading(level) => lines(text, &selection, Prefix::Heading(level.clamp(1, 6))),
        Format::BulletList => lines(text, &selection, Prefix::Bullet),
        Format::NumberedList => lines(text, &selection, Prefix::Numbered),
        Format::TaskList => lines(text, &selection, Prefix::Task),
        Format::Quote => lines(text, &selection, Prefix::Quote),
    }
}

fn clamp(text: &str, selection: Range<usize>) -> Range<usize> {
    let floor = |at: usize| {
        let mut at = at.min(text.len());
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        if text[..at].ends_with('\r') && text[at..].starts_with('\n') {
            at -= 1;
        }
        at
    };
    let (a, b) = (floor(selection.start), floor(selection.end));
    a.min(b)..a.max(b)
}

fn eol(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn run(bytes: impl Iterator<Item = u8>, byte: u8) -> usize {
    bytes.take_while(|&b| b == byte).count()
}

fn inline(text: &str, sel: Range<usize>, marker: &str) -> Edit {
    let m = marker.len();
    let byte = marker.as_bytes()[0];
    // A run of `*` holds italic when odd: `**` is bold and `***` is both.
    let styled = |run: usize| {
        if marker == "*" {
            run % 2 == 1
        } else {
            run >= m
        }
    };
    let inner = &text[sel.clone()];
    let outside =
        run(text[..sel.start].bytes().rev(), byte).min(run(text[sel.end..].bytes(), byte));
    if styled(outside) {
        return Edit {
            range: sel.start - m..sel.end + m,
            text: inner.to_owned(),
            selection: sel.start - m..sel.end - m,
        };
    }
    let lead = run(inner.bytes(), byte);
    if lead < inner.len() && styled(lead.min(run(inner.bytes().rev(), byte))) {
        return Edit {
            text: inner[m..inner.len() - m].to_owned(),
            selection: sel.start..sel.end - 2 * m,
            range: sel,
        };
    }
    Edit {
        text: format!("{marker}{inner}{marker}"),
        selection: sel.start + m..sel.end + m,
        range: sel,
    }
}

fn link(text: &str, sel: Range<usize>) -> Edit {
    let label = if sel.is_empty() {
        "text"
    } else {
        &text[sel.clone()]
    };
    let new = format!("[{label}](url)");
    let selection = if sel.is_empty() {
        sel.start + 1..sel.start + 1 + label.len()
    } else {
        let url = sel.start + new.len() - "url)".len();
        url..url + "url".len()
    };
    Edit {
        range: sel,
        text: new,
        selection,
    }
}

/// The lines `sel` touches, without the final line end. A selection that
/// ends at the start of a line doesn't touch that line.
fn line_span(text: &str, sel: &Range<usize>) -> Range<usize> {
    let start = text[..sel.start].rfind('\n').map_or(0, |at| at + 1);
    let mut last = sel.end;
    if sel.end > sel.start && text[..last].ends_with('\n') {
        last -= 1;
    }
    let mut end = text[last..].find('\n').map_or(text.len(), |at| last + at);
    if text[..end].ends_with('\r') {
        end -= 1;
    }
    start..end
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prefix {
    Heading(u8),
    Quote,
    Bullet,
    Numbered,
    Task,
}

impl Prefix {
    fn is_list(self) -> bool {
        matches!(self, Prefix::Bullet | Prefix::Numbered | Prefix::Task)
    }
}

/// The line's block prefix, its indent and the length of its marker.
fn parse(row: &str) -> (Option<Prefix>, usize, usize) {
    let hashes = run(row.bytes(), b'#');
    if let Ok(level @ 1..=6) = u8::try_from(hashes)
        && (row.len() == hashes || row[hashes..].starts_with(' '))
    {
        return (Some(Prefix::Heading(level)), 0, (hashes + 1).min(row.len()));
    }
    if let Some(rest) = row.strip_prefix('>') {
        return (
            Some(Prefix::Quote),
            0,
            1 + usize::from(rest.starts_with(' ')),
        );
    }
    let indent = row.len() - row.trim_start_matches([' ', '\t']).len();
    let body = &row[indent..];
    if let Some(rest) = body
        .strip_prefix(['-', '*', '+'])
        .and_then(|rest| rest.strip_prefix(' '))
    {
        return match rest.get(..4) {
            Some("[ ] " | "[x] " | "[X] ") => (Some(Prefix::Task), indent, 6),
            _ => (Some(Prefix::Bullet), indent, 2),
        };
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && (body[digits..].starts_with(". ") || body[digits..].starts_with(") ")) {
        return (Some(Prefix::Numbered), indent, digits + 2);
    }
    (None, indent, 0)
}

fn lines(text: &str, sel: &Range<usize>, target: Prefix) -> Edit {
    let span = line_span(text, sel);
    let rows: Vec<&str> = text[span.clone()].split('\n').collect();
    let skip = |row: &str| rows.len() > 1 && row.trim().is_empty();
    let off = rows
        .iter()
        .all(|row| skip(row) || parse(row.trim_end_matches('\r')).0 == Some(target));
    let mut out = String::new();
    let mut number = 0;
    let mut cursor = sel.start;
    for (i, raw) in rows.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if skip(raw) {
            out.push_str(raw);
            continue;
        }
        let row = raw.trim_end_matches('\r');
        let (kind, indent, len) = parse(row);
        let (cut, keep) = match (kind, target) {
            (_, t) if t.is_list() => (
                indent
                    + if kind.is_some_and(Prefix::is_list) {
                        len
                    } else {
                        0
                    },
                &row[..indent],
            ),
            (Some(Prefix::Heading(_)), Prefix::Heading(_)) => (len, ""),
            (Some(Prefix::Quote), Prefix::Quote) if off => (len, ""),
            _ => (0, ""),
        };
        let mark = match target {
            _ if off => String::new(),
            Prefix::Heading(level) => format!("{} ", "#".repeat(level.into())),
            Prefix::Quote => "> ".to_owned(),
            Prefix::Bullet => "- ".to_owned(),
            Prefix::Task => "- [ ] ".to_owned(),
            Prefix::Numbered => {
                number += 1;
                format!("{number}. ")
            }
        };
        let prefix = format!("{keep}{mark}");
        cursor =
            span.start + out.len() + prefix.len() + (sel.start - span.start).saturating_sub(cut);
        out.push_str(&prefix);
        out.push_str(&raw[cut..]);
    }
    let selection = if sel.is_empty() {
        cursor..cursor
    } else {
        span.start..span.start + out.len()
    };
    Edit {
        range: span,
        text: out,
        selection,
    }
}

fn code_block(text: &str, sel: Range<usize>) -> Edit {
    let eol = eol(text);
    if sel.is_empty() {
        let at = sel.start;
        let lead = if at == 0 || text[..at].ends_with('\n') {
            ""
        } else {
            eol
        };
        let trail = if text[at..].is_empty() || text[at..].starts_with(['\r', '\n']) {
            ""
        } else {
            eol
        };
        let cursor = at + lead.len() + "```".len() + eol.len();
        return Edit {
            range: at..at,
            text: format!("{lead}```{eol}{eol}```{trail}"),
            selection: cursor..cursor,
        };
    }
    let span = line_span(text, &sel);
    let body = &text[span.clone()];
    let start = span.start + "```".len() + eol.len();
    Edit {
        text: format!("```{eol}{body}{eol}```"),
        selection: start..start + body.len(),
        range: span,
    }
}

fn table(text: &str, sel: &Range<usize>) -> Edit {
    let eol = eol(text);
    let span = line_span(text, &(sel.end..sel.end));
    let (at, lead) = if span.is_empty() {
        (span.start, String::new())
    } else {
        (span.end, eol.repeat(2))
    };
    let trail = if span.end < text.len() { eol } else { "" };
    let cell = at + lead.len() + "| ".len();
    Edit {
        range: at..at,
        text: format!("{lead}| Header | Header |{eol}| --- | --- |{eol}| Cell | Cell |{trail}"),
        selection: cell..cell + "Header".len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text after the edit, and what it selects.
    fn toolbar(format: Format, text: &str, selection: Range<usize>) -> (String, String) {
        let edit = apply(format, text, selection);
        let mut new = text.to_owned();
        new.replace_range(edit.range, &edit.text);
        let selected = new[edit.selection].to_owned();
        (new, selected)
    }

    fn find(text: &str, needle: &str) -> Range<usize> {
        let at = text.find(needle).unwrap();
        at..at + needle.len()
    }

    #[test]
    fn inline_styles_wrap_and_toggle_off() {
        let text = "say hi now";
        for (format, m) in [
            (Format::Bold, "**"),
            (Format::Italic, "*"),
            (Format::Strikethrough, "~~"),
            (Format::InlineCode, "`"),
        ] {
            let (styled, selected) = toolbar(format, text, find(text, "hi"));
            assert_eq!(styled, format!("say {m}hi{m} now"));
            assert_eq!(selected, "hi");
            assert_eq!(
                toolbar(format, &styled, find(&styled, "hi")),
                (text.into(), "hi".into())
            );
            let marked = format!("{m}hi{m}");
            assert_eq!(
                toolbar(format, &styled, find(&styled, &marked)),
                (text.into(), "hi".into())
            );

            let edit = apply(format, "ab", 1..1);
            assert_eq!(edit.text, format!("{m}{m}"));
            assert_eq!(edit.selection, 1 + m.len()..1 + m.len());
            assert_eq!(
                toolbar(format, &format!("a{m}{m}b"), edit.selection),
                ("ab".into(), String::new())
            );
        }
    }

    #[test]
    fn italic_and_bold_nest_rather_than_strip_each_other() {
        let (both, _) = toolbar(Format::Italic, "**hi**", 2..4);
        assert_eq!(both, "***hi***");
        assert_eq!(toolbar(Format::Bold, &both, 3..5).0, "*hi*");
        assert_eq!(toolbar(Format::Italic, &both, 3..5).0, "**hi**");
    }

    #[test]
    fn link_selects_what_to_type_next() {
        assert_eq!(
            toolbar(Format::Link, "see docs", 4..8),
            ("see [docs](url)".into(), "url".into())
        );
        assert_eq!(
            toolbar(Format::Link, "see ", 4..4),
            ("see [text](url)".into(), "text".into())
        );
    }

    #[test]
    fn line_tools_prefix_every_touched_line_and_toggle_back() {
        let text = "intro\none\ntwo\n\nthree\noutro";
        let sel = text.find("ne").unwrap()..text.find("hr").unwrap();
        for (format, block) in [
            (Format::BulletList, "- one\n- two\n\n- three"),
            (Format::TaskList, "- [ ] one\n- [ ] two\n\n- [ ] three"),
            (Format::NumberedList, "1. one\n2. two\n\n3. three"),
            (Format::Quote, "> one\n> two\n\n> three"),
            (Format::Heading(2), "## one\n## two\n\n## three"),
        ] {
            let (new, selected) = toolbar(format, text, sel.clone());
            assert_eq!(new, format!("intro\n{block}\noutro"));
            assert_eq!(selected, block);
            let again = find(&new, block);
            assert_eq!(toolbar(format, &new, again).0, text);
        }
    }

    #[test]
    fn a_list_tool_replaces_another_list_marker() {
        let text = "- a\n  * b\n3) c";
        let (numbered, _) = toolbar(Format::NumberedList, text, 0..text.len());
        assert_eq!(numbered, "1. a\n  2. b\n3. c");
        let (tasks, _) = toolbar(Format::TaskList, &numbered, 0..numbered.len());
        assert_eq!(tasks, "- [ ] a\n  - [ ] b\n- [ ] c");
        let (bullets, _) = toolbar(Format::BulletList, &tasks, 0..tasks.len());
        assert_eq!(bullets, "- a\n  - b\n- c");
    }

    #[test]
    fn a_partly_styled_selection_gets_styled_not_cleared() {
        assert_eq!(toolbar(Format::Quote, "> a\nb", 0..5).0, "> > a\n> b");
        assert_eq!(toolbar(Format::BulletList, "- a\nb", 0..5).0, "- a\n- b");
    }

    #[test]
    fn heading_changes_level_and_the_cursor_stays_on_its_character() {
        let edit = apply(Format::Heading(2), "# Title", 4..4);
        assert_eq!(edit.text, "## Title");
        assert_eq!(edit.selection, 5..5);
        let edit = apply(Format::Heading(2), "## Title", 5..5);
        assert_eq!(edit.text, "Title");
        assert_eq!(edit.selection, 2..2);
        let edit = apply(Format::BulletList, "a\n\nb", 2..2);
        assert_eq!(edit.text, "- ");
        assert_eq!(edit.selection, 4..4);
    }

    #[test]
    fn code_block_fences_the_touched_lines() {
        assert_eq!(
            toolbar(Format::CodeBlock, "a\nbc\nd", 3..4),
            ("a\n```\nbc\n```\nd".into(), "bc".into())
        );
        let edit = apply(Format::CodeBlock, "a", 1..1);
        assert_eq!(edit.text, "\n```\n\n```");
        assert_eq!(edit.selection, 6..6);
    }

    #[test]
    fn table_goes_on_its_own_lines_with_the_first_header_selected() {
        let (new, selected) = toolbar(Format::Table, "Intro\nnext", 2..2);
        assert_eq!(
            new,
            "Intro\n\n| Header | Header |\n| --- | --- |\n| Cell | Cell |\n\nnext"
        );
        assert_eq!(selected, "Header");
        assert!(
            new[apply(Format::Table, "Intro\nnext", 2..2).selection.start..]
                .starts_with("Header | Header")
        );
        assert_eq!(
            toolbar(Format::Table, "", 0..0).0,
            "| Header | Header |\n| --- | --- |\n| Cell | Cell |"
        );
    }

    #[test]
    fn crlf_lines_keep_their_line_ends() {
        let text = "one\r\ntwo\r\n";
        assert_eq!(
            toolbar(Format::BulletList, text, 0..text.len()).0,
            "- one\r\n- two\r\n"
        );
        assert_eq!(
            toolbar(Format::CodeBlock, text, 0..text.len()).0,
            "```\r\none\r\ntwo\r\n```\r\n"
        );
        assert_eq!(apply(Format::Quote, text, 4..4).text, "> one");
    }

    #[test]
    fn every_tool_on_every_selection_yields_a_valid_edit() {
        let sample = "# Tïtle\r\n- [ ] 日本語 **b**\r\n\r\n> q 🎉\n12) `c`\r\n*";
        let formats = [
            Format::Heading(0),
            Format::Heading(3),
            Format::Heading(200),
            Format::Bold,
            Format::Italic,
            Format::Strikethrough,
            Format::InlineCode,
            Format::CodeBlock,
            Format::Link,
            Format::BulletList,
            Format::NumberedList,
            Format::TaskList,
            Format::Quote,
            Format::Table,
        ];
        for start in 0..sample.len() + 3 {
            for end in 0..sample.len() + 3 {
                for format in formats {
                    let edit = apply(format, sample, start..end);
                    let mut new = sample.to_owned();
                    new.replace_range(edit.range.clone(), &edit.text);
                    let sel = edit.selection;
                    assert!(
                        sel.start <= sel.end && sel.end <= new.len(),
                        "{format:?} {start}..{end}"
                    );
                    assert!(new.is_char_boundary(sel.start) && new.is_char_boundary(sel.end));
                }
            }
        }
    }
}
