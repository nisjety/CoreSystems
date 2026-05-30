//! Change detection: page-level fingerprint compare + paragraph-level
//! semantic-ish diff.
//!
//! "Semantic-ish" because we operate on paragraph fingerprints, not embeddings:
//! the diff highlights which paragraphs were added, removed, or merely
//! renumbered. It is sufficient for `change.detected` event payloads and for
//! scoring change-detection precision in Phase 8 evals.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use quarry_core::output::ChangeStatus;

use crate::chunk::{paragraph_chunks, Chunk};
use crate::fingerprint::Fingerprint;

pub fn compare(prev: Option<&Fingerprint>, current: &Fingerprint) -> ChangeStatus {
    match prev {
        None => ChangeStatus::New,
        Some(p) if p == current => ChangeStatus::Unchanged,
        Some(_) => ChangeStatus::Changed,
    }
}

/// One unit of paragraph-level change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DiffOp {
    /// Paragraph appears in `current` but not in `prev`.
    Added {
        index: usize,
        fingerprint: String,
        text: String,
    },
    /// Paragraph appears in `prev` but not in `current`.
    Removed {
        index: usize,
        fingerprint: String,
        text: String,
    },
    /// Same fingerprint present at a different index — content is unchanged
    /// but its position shifted (typical of inserted intros, dropped headers).
    Moved {
        from: usize,
        to: usize,
        fingerprint: String,
    },
}

/// Result of paragraph-level diff. `is_real_delta` is `false` when the only
/// changes are paragraphs being moved (no add/remove).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticDiff {
    pub ops: Vec<DiffOp>,
    pub added: usize,
    pub removed: usize,
    pub moved: usize,
    pub unchanged: usize,
}

impl SemanticDiff {
    /// Real delta = at least one paragraph added or removed. Pure reorderings
    /// are not surfaced as `change.detected` to reduce churn.
    pub fn is_real_delta(&self) -> bool {
        self.added > 0 || self.removed > 0
    }
}

/// Produce a paragraph-level diff between two markdown/plaintext bodies.
/// `max_chars` controls paragraph chunk size (matches `paragraph_chunks`).
pub fn diff_paragraphs(prev: &str, current: &str, max_chars: usize) -> SemanticDiff {
    let prev_chunks = paragraph_chunks(prev, max_chars);
    let curr_chunks = paragraph_chunks(current, max_chars);

    let prev_fps: Vec<String> = prev_chunks.iter().map(fp).collect();
    let curr_fps: Vec<String> = curr_chunks.iter().map(fp).collect();

    let prev_set: HashSet<&String> = prev_fps.iter().collect();
    let curr_set: HashSet<&String> = curr_fps.iter().collect();

    let mut ops = Vec::new();
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut moved = 0usize;
    let mut unchanged = 0usize;

    for (i, (chunk, hash)) in curr_chunks.iter().zip(curr_fps.iter()).enumerate() {
        if !prev_set.contains(hash) {
            ops.push(DiffOp::Added {
                index: i,
                fingerprint: hash.clone(),
                text: chunk.text.clone(),
            });
            added += 1;
            continue;
        }
        // Same content; check if it moved.
        let prev_index = prev_fps.iter().position(|h| h == hash);
        match prev_index {
            Some(j) if j != i => {
                ops.push(DiffOp::Moved {
                    from: j,
                    to: i,
                    fingerprint: hash.clone(),
                });
                moved += 1;
            }
            _ => unchanged += 1,
        }
    }
    for (j, (chunk, hash)) in prev_chunks.iter().zip(prev_fps.iter()).enumerate() {
        if !curr_set.contains(hash) {
            ops.push(DiffOp::Removed {
                index: j,
                fingerprint: hash.clone(),
                text: chunk.text.clone(),
            });
            removed += 1;
        }
    }

    SemanticDiff {
        ops,
        added,
        removed,
        moved,
        unchanged,
    }
}

/// Hash a chunk's normalized text. Whitespace is collapsed before hashing so
/// reflowed paragraphs (different line breaks, same words) match.
fn fp(c: &Chunk) -> String {
    let normalized: String = c.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let h = blake3::hash(normalized.as_bytes());
    h.to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_returns_no_real_delta() {
        let d = diff_paragraphs("alpha\n\nbeta\n\ngamma", "alpha\n\nbeta\n\ngamma", 256);
        assert_eq!(d.added, 0);
        assert_eq!(d.removed, 0);
        assert!(!d.is_real_delta());
        assert_eq!(d.unchanged, 3);
    }

    #[test]
    fn added_paragraph_marks_real_delta() {
        let prev = "alpha\n\nbeta";
        let curr = "alpha\n\nbeta\n\ngamma";
        let d = diff_paragraphs(prev, curr, 256);
        assert_eq!(d.added, 1);
        assert_eq!(d.removed, 0);
        assert!(d.is_real_delta());
    }

    #[test]
    fn removed_paragraph_marks_real_delta() {
        let d = diff_paragraphs("alpha\n\nbeta\n\ngamma", "alpha\n\ngamma", 256);
        assert_eq!(d.added, 0);
        assert_eq!(d.removed, 1);
        assert!(d.is_real_delta());
    }

    #[test]
    fn pure_reorder_is_not_a_real_delta() {
        let d = diff_paragraphs("alpha\n\nbeta\n\ngamma", "gamma\n\nalpha\n\nbeta", 256);
        assert_eq!(d.added, 0);
        assert_eq!(d.removed, 0);
        assert!(d.moved >= 1);
        assert!(!d.is_real_delta());
    }

    #[test]
    fn whitespace_only_changes_are_not_a_delta() {
        let d = diff_paragraphs("alpha beta", "alpha\nbeta", 256);
        assert_eq!(d.added, 0);
        assert_eq!(d.removed, 0);
        assert!(!d.is_real_delta());
    }

    #[test]
    fn ops_contain_added_text() {
        let d = diff_paragraphs("alpha", "alpha\n\nnew_para", 256);
        assert!(d
            .ops
            .iter()
            .any(|op| matches!(op, DiffOp::Added { text, .. } if text.contains("new_para"))));
    }
}
