//! Reading one file of a skill bundle: path safety, a read cap, and paging.
//!
//! # Three stages of disclosure
//!
//! A matched skill reaches the prompt in two stages: its instruction text, and
//! a footer that says its bundle exists. The bundle's own files reach the model
//! only when it asks for one, and then only a bounded piece at a time. A bundle
//! can hold 256 files and 8 MiB, so injecting it would spend the room the skill
//! budget protects; a footer costs one line.
//!
//! # Why this lives in the contracts crate
//!
//! What a bundle path may name, and how much of a file one read may return, are
//! rules the bundle store and every reader must agree on. This module is
//! std-only and works on strings and bytes, so it stays usable wherever the
//! generated proto types are not.

/// Most bytes one read returns. A reference document fits in a few reads, and a
/// model that walks a whole bundle cannot fill the context by itself.
pub const MAX_READ_BYTES: usize = 32 * 1024;

/// Environment switch for the `skill_file` tool. Only the literal `true`
/// enables it, so an unset, empty or mistyped value leaves chat as it was.
pub const SKILL_FILE_ENV: &str = "SKILL_FILE_TOOL_ENABLED";

/// Whether `raw` (the value of [`SKILL_FILE_ENV`], if set) is the literal on-word.
#[must_use]
pub fn enabled_from(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether the `skill_file` tool is switched on in this process.
#[must_use]
pub fn enabled() -> bool {
    enabled_from(std::env::var(SKILL_FILE_ENV).ok().as_deref())
}

/// Why a path was refused as a bundle path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRefusal {
    /// Nothing was named.
    Empty,
    /// Rooted at a separator, or carrying a drive letter or a stream marker, so
    /// it does not name a place inside the bundle.
    NotRelative,
    /// A `.` or `..` segment, which can name a place outside the bundle.
    Traversal,
    /// A backslash, an empty segment or a control character: no manifest holds
    /// a name spelled that way.
    Malformed,
}

impl PathRefusal {
    /// The reason, in words the model can act on.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::Empty => "no path was given",
            Self::NotRelative => {
                "the path must be relative to the bundle, with no leading slash and no drive letter"
            }
            Self::Traversal => "the path may not use '.' or '..' segments",
            Self::Malformed => {
                "the path must use forward slashes, with no empty segments or control characters"
            }
        }
    }
}

/// Check that `path` names a file inside a bundle, spelled as a manifest spells it.
///
/// # Errors
///
/// Returns the first [`PathRefusal`] that applies.
pub fn validate_bundle_path(path: &str) -> Result<(), PathRefusal> {
    if path.is_empty() {
        return Err(PathRefusal::Empty);
    }
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return Err(PathRefusal::NotRelative);
    }
    if path
        .split(['/', '\\'])
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(PathRefusal::Traversal);
    }
    if path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').any(str::is_empty)
    {
        return Err(PathRefusal::Malformed);
    }
    Ok(())
}

/// What one read of a file returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Text from the requested offset, at most [`MAX_READ_BYTES`]. `next_offset`
    /// is where the following read must start when the file goes on.
    Text {
        content: String,
        next_offset: Option<usize>,
    },
    /// Not text (a NUL byte, or bytes that are not UTF-8). No content is returned.
    Binary,
}

/// Why a read could not start where it was asked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetRefusal {
    /// The offset lies beyond the end of the file.
    PastEnd,
    /// The offset falls inside a multi-byte character. Every `next_offset` a read
    /// returns is on a boundary, and that is the offset to pass on.
    NotCharBoundary,
}

/// Read up to [`MAX_READ_BYTES`] of `content` from `offset`, never splitting a
/// character.
///
/// Binary content is reported before the offset is checked: a caller asking for
/// the middle of a picture gets told it is binary, not that its offset is wrong.
///
/// # Errors
///
/// Returns [`OffsetRefusal`] when `offset` is past the end, or inside a character.
pub fn read_piece(content: &[u8], offset: usize) -> Result<Piece, OffsetRefusal> {
    let text = match std::str::from_utf8(content) {
        Ok(text) if !content.contains(&0) => text,
        _ => return Ok(Piece::Binary),
    };
    if offset > text.len() {
        return Err(OffsetRefusal::PastEnd);
    }
    if !text.is_char_boundary(offset) {
        return Err(OffsetRefusal::NotCharBoundary);
    }
    let mut end = text.len().min(offset.saturating_add(MAX_READ_BYTES));
    // A character is at most four bytes, so this backs off at most three.
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(Piece::Text {
        content: text[offset..end].to_owned(),
        next_offset: (end < text.len()).then_some(end),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_relative_path_is_accepted() {
        assert_eq!(validate_bundle_path("SKILL.md"), Ok(()));
        assert_eq!(validate_bundle_path("references/guide.md"), Ok(()));
        assert_eq!(validate_bundle_path("scripts/run.py"), Ok(()));
    }

    #[test]
    fn traversal_is_refused_however_it_is_spelled() {
        for path in [
            "..",
            "../secret",
            "references/../../secret",
            "a/./b",
            "references\\..\\secret",
        ] {
            assert_eq!(
                validate_bundle_path(path),
                Err(PathRefusal::Traversal),
                "{path}"
            );
        }
    }

    #[test]
    fn rooted_and_drive_paths_are_refused() {
        for path in [
            "/etc/passwd",
            "\\\\server\\share",
            "C:/x",
            "C:\\x",
            "file:stream",
        ] {
            assert_eq!(
                validate_bundle_path(path),
                Err(PathRefusal::NotRelative),
                "{path}"
            );
        }
    }

    #[test]
    fn malformed_names_are_refused() {
        for path in ["a//b", "a/", "a\\b", "a\nb", "a\u{0}b"] {
            assert_eq!(
                validate_bundle_path(path),
                Err(PathRefusal::Malformed),
                "{path:?}"
            );
        }
        assert_eq!(validate_bundle_path(""), Err(PathRefusal::Empty));
    }

    #[test]
    fn a_text_file_under_the_cap_comes_back_whole() {
        assert_eq!(
            read_piece(b"hello", 0),
            Ok(Piece::Text {
                content: "hello".to_owned(),
                next_offset: None,
            })
        );
    }

    #[test]
    fn a_large_file_pages_at_the_cap_and_names_where_the_next_piece_starts() {
        let content = "a".repeat(2 * MAX_READ_BYTES + 10);
        let mut offset = 0;
        let mut rebuilt = String::new();
        let mut sizes = Vec::new();
        loop {
            let Ok(Piece::Text {
                content: piece,
                next_offset,
            }) = read_piece(content.as_bytes(), offset)
            else {
                panic!("a text file must read as text");
            };
            sizes.push(piece.len());
            rebuilt.push_str(&piece);
            match next_offset {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(sizes, vec![MAX_READ_BYTES, MAX_READ_BYTES, 10]);
        assert_eq!(rebuilt, content);
    }

    #[test]
    fn paging_never_splits_a_multi_byte_character() {
        // Three-byte characters: the cap lands inside one, so the first read
        // backs off to the boundary and says where the next one starts.
        let content = "€".repeat(20_000);
        let Ok(Piece::Text {
            content: first,
            next_offset,
        }) = read_piece(content.as_bytes(), 0)
        else {
            panic!("a text file must read as text");
        };
        assert_eq!(next_offset, Some(32_766));
        assert_eq!(first.len(), 32_766);
        let Ok(Piece::Text {
            content: second,
            next_offset: after,
        }) = read_piece(content.as_bytes(), 32_766)
        else {
            panic!("a text file must read as text");
        };
        assert_eq!(after, None);
        assert_eq!(format!("{first}{second}"), content);
    }

    #[test]
    fn an_offset_inside_a_character_or_past_the_end_is_refused() {
        let content = "é".as_bytes();
        assert_eq!(read_piece(content, 1), Err(OffsetRefusal::NotCharBoundary));
        assert_eq!(read_piece(content, 3), Err(OffsetRefusal::PastEnd));
        assert_eq!(
            read_piece(content, 2),
            Ok(Piece::Text {
                content: String::new(),
                next_offset: None,
            })
        );
    }

    #[test]
    fn binary_content_returns_no_text_whatever_the_offset() {
        assert_eq!(
            read_piece(&[0x89, b'P', b'N', b'G', 0, 1], 0),
            Ok(Piece::Binary)
        );
        assert_eq!(read_piece(&[0xff, 0xfe], 0), Ok(Piece::Binary));
        assert_eq!(read_piece(&[0xff], 99), Ok(Piece::Binary));
    }

    #[test]
    fn only_the_literal_true_enables_the_tool() {
        assert!(enabled_from(Some("true")));
        for raw in [
            None,
            Some(""),
            Some("True"),
            Some(" true"),
            Some("1"),
            Some("yes"),
        ] {
            assert!(!enabled_from(raw), "{raw:?}");
        }
    }
}
