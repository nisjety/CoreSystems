//! Recursive, structure-aware text chunker.
//!
//! Contextual Retrieval is NOT here. An earlier version of this header claimed
//! it was "wired behind flags in config"; there were no such flags, and this was
//! never the right home for it — `builder::process_document` runs its whole
//! build inside one transaction holding `FOR UPDATE` row locks, so per-chunk LLM
//! calls would hold those locks for minutes, and this module is deliberately
//! pure and synchronous with no network client. It now lives in
//! `embedding-engine-rs::provider::contextualize`, which already holds an
//! org-bound inference credential, runs off the transaction critical path, and
//! is the layer that decides what text gets embedded. DI-layout remains
//! genuinely unbuilt, pending a Document Intelligence resource.
//!
//! Strategy (PR-E, phased — structural pass):
//!   1. Split into structural segments. **Atomic** blocks — markdown tables and
//!      fenced code — are kept whole (never sentence-split or paragraph-split), so
//!      a table's rows can't be scattered across chunks (the single biggest recall
//!      killer for structured content). Everything else is a paragraph segment.
//!   2. A non-atomic paragraph that exceeds the token budget is recursively split
//!      into sentences.
//!   3. Greedily pack segments into chunks within the token budget, carrying a
//!      token-bounded overlap tail between chunks.
//!   4. A single segment larger than the budget (a huge table/code block, or one
//!      very long sentence) is window-split on character boundaries as a last
//!      resort.
//!
//! Token counts use a char-based estimate (4 chars ≈ 1 token for cl100k_base).
//!
//! **Parent-child windows (P2-2, closes the chunking half of D13 — int8
//! quantization already shipped separately in
//! `embedding-engine-rs::qdrant_writer`).** `attach_parent_windows` is a
//! second, optional pass over the child chunks this module already produces:
//! each child keeps its own (small, precise) text for embedding, and gains a
//! wider `parent_text` window of *neighboring* chunks for retrieval-time
//! context expansion. Off by default (`ChunkConfig::parent_chunk_size` is
//! `None`) — this changes what gets persisted for every future chunk, and
//! nothing downstream consumes `parent_text` yet (retrieval-engine-rs does
//! not read it), so it stays inert-but-tested until a caller does. Same
//! shape as `query_expansion` (D14) sitting accepted-but-unconsumed before
//! P2-7 wired it up.

#[derive(Debug, Clone)]
pub struct ChunkConfig {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    /// Token budget for each child's parent window. `None` disables parent-
    /// window attachment entirely (the default) — every `Chunk.parent_text`
    /// stays `None` and `chunk_text`'s output is byte-for-byte what it always
    /// was.
    pub parent_chunk_size: Option<usize>,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            chunk_overlap: 64,
            parent_chunk_size: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: usize,
    pub text: String,
    pub estimated_tokens: usize,
    /// Wider neighboring-chunk context for retrieval-time expansion. Always
    /// `None` from `chunk_text` itself — populated only by
    /// `attach_parent_windows`, kept as a distinct pass so the (already
    /// well-tested) structural chunking above stays unchanged either way.
    pub parent_text: Option<String>,
}

/// A structural segment. `atomic` blocks (tables, fenced code) are never split by
/// sentence/paragraph logic — only window-split if they alone exceed the budget.
#[derive(Debug, Clone)]
struct Segment {
    text: String,
    atomic: bool,
}

pub fn chunk_text(text: &str, config: &ChunkConfig) -> Vec<Chunk> {
    if text.trim().is_empty() {
        return vec![];
    }

    // 1 + 2: structural split, then recursively sentence-split oversized prose.
    let mut segments: Vec<Segment> = Vec::new();
    for seg in split_structural(text) {
        if !seg.atomic && estimate_tokens(&seg.text) > config.chunk_size {
            for sentence in split_sentences(&seg.text) {
                let trimmed = sentence.trim();
                if !trimmed.is_empty() {
                    segments.push(Segment {
                        text: trimmed.to_string(),
                        atomic: false,
                    });
                }
            }
        } else {
            segments.push(seg);
        }
    }

    // 3 + 4: greedy pack with overlap; window-split any single oversized segment.
    let mut chunks: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_tokens: usize = 0;

    for segment in &segments {
        let seg_tokens = estimate_tokens(&segment.text);

        if seg_tokens > config.chunk_size {
            if !current.is_empty() {
                chunks.push(current.join(" "));
                current.clear();
                current_tokens = 0;
            }
            let step = config
                .chunk_size
                .saturating_sub(config.chunk_overlap)
                .max(1);
            let chars: Vec<char> = segment.text.chars().collect();
            let char_step = step * 4;
            let char_size = config.chunk_size * 4;
            let mut start = 0;
            while start < chars.len() {
                let end = (start + char_size).min(chars.len());
                let window: String = chars[start..end].iter().collect();
                chunks.push(window);
                start += char_step;
            }
            continue;
        }

        if current_tokens + seg_tokens > config.chunk_size && !current.is_empty() {
            chunks.push(current.join(" "));

            // Keep a token-bounded trailing window for overlap.
            let mut overlap_segments: Vec<String> = Vec::new();
            let mut overlap_tokens: usize = 0;
            for seg in current.iter().rev() {
                let seg_t = estimate_tokens(seg);
                if overlap_tokens + seg_t > config.chunk_overlap {
                    break;
                }
                overlap_segments.insert(0, seg.clone());
                overlap_tokens += seg_t;
            }

            current = overlap_segments;
            current.push(segment.text.clone());
            current_tokens = current.iter().map(|s| estimate_tokens(s)).sum();
        } else {
            current.push(segment.text.clone());
            current_tokens += seg_tokens;
        }
    }

    if !current.is_empty() {
        chunks.push(current.join(" "));
    }

    let chunks: Vec<Chunk> = chunks
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let estimated_tokens = estimate_tokens(&text);
            Chunk {
                index,
                text,
                estimated_tokens,
                parent_text: None,
            }
        })
        .collect();

    match config.parent_chunk_size {
        Some(budget) => attach_parent_windows(chunks, budget),
        None => chunks,
    }
}

/// Give every child chunk a wider `parent_text` window: neighboring chunks
/// (both directions) expanded outward from the child's own position until
/// `parent_chunk_size` (a token budget, same units as `chunk_size`) is
/// reached or the document's chunk list is exhausted. A single-chunk
/// document has no neighbors to borrow, so it is returned unchanged.
fn attach_parent_windows(chunks: Vec<Chunk>, parent_chunk_size: usize) -> Vec<Chunk> {
    if chunks.len() <= 1 {
        return chunks;
    }
    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
    let windows: Vec<String> = (0..chunks.len())
        .map(|i| expand_window(&texts, i, parent_chunk_size))
        .collect();
    // `windows` no longer borrows from `chunks` (each entry is an owned
    // `String`), so `chunks` is free to move into the zip below.
    chunks
        .into_iter()
        .zip(windows)
        .map(|(mut chunk, window)| {
            chunk.parent_text = Some(window);
            chunk
        })
        .collect()
}

/// Grows a `[lo, hi]` window outward from `center`, alternating sides so it
/// stays roughly symmetric, stopping as soon as the next candidate side would
/// push the running token estimate past `budget`. Stopping on the first
/// blocked side (rather than trying the other side first) slightly
/// undersizes some windows near a document's edge, which is an acceptable
/// simplification: the child chunk itself is always included regardless of
/// budget, so `parent_text` is never empty or truncated *below* the child.
fn expand_window(texts: &[&str], center: usize, budget: usize) -> String {
    let mut lo = center;
    let mut hi = center;
    let mut tokens = estimate_tokens(texts[center]);
    let mut prefer_left = true;

    loop {
        let can_left = lo > 0;
        let can_right = hi + 1 < texts.len();
        if !can_left && !can_right {
            break;
        }
        let go_left = if can_left && can_right {
            prefer_left
        } else {
            can_left
        };
        let next_text = if go_left {
            texts[lo - 1]
        } else {
            texts[hi + 1]
        };
        let next_tokens = estimate_tokens(next_text);
        if tokens + next_tokens > budget {
            break;
        }
        tokens += next_tokens;
        if go_left {
            lo -= 1;
        } else {
            hi += 1;
        }
        prefer_left = !prefer_left;
    }

    texts[lo..=hi].join(" ")
}

/// Split raw text into structural segments, marking markdown tables and fenced
/// code blocks as atomic so downstream packing never tears them apart.
fn split_structural(text: &str) -> Vec<Segment> {
    let lines: Vec<&str> = text.lines().collect();
    let mut segments: Vec<Segment> = Vec::new();
    let mut paragraph = String::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let leading = line.trim_start();

        // Fenced code block: ``` … ``` kept whole.
        if leading.starts_with("```") {
            flush_paragraph(&mut paragraph, &mut segments);
            let mut block = String::new();
            block.push_str(line);
            block.push('\n');
            i += 1;
            while i < lines.len() {
                block.push_str(lines[i]);
                block.push('\n');
                let closes = lines[i].trim_start().starts_with("```");
                i += 1;
                if closes {
                    break;
                }
            }
            push_segment(&mut segments, block.trim_end(), true);
            continue;
        }

        // Markdown table: consecutive lines starting with '|' kept whole.
        if leading.starts_with('|') {
            flush_paragraph(&mut paragraph, &mut segments);
            let mut block = String::new();
            while i < lines.len() && lines[i].trim_start().starts_with('|') {
                block.push_str(lines[i]);
                block.push('\n');
                i += 1;
            }
            push_segment(&mut segments, block.trim_end(), true);
            continue;
        }

        // Blank line → paragraph boundary.
        if line.trim().is_empty() {
            flush_paragraph(&mut paragraph, &mut segments);
            i += 1;
            continue;
        }

        if !paragraph.is_empty() {
            paragraph.push('\n');
        }
        paragraph.push_str(line);
        i += 1;
    }

    flush_paragraph(&mut paragraph, &mut segments);
    segments
}

fn flush_paragraph(paragraph: &mut String, segments: &mut Vec<Segment>) {
    let trimmed = paragraph.trim();
    if !trimmed.is_empty() {
        segments.push(Segment {
            text: trimmed.to_string(),
            atomic: false,
        });
    }
    paragraph.clear();
}

fn push_segment(segments: &mut Vec<Segment>, text: &str, atomic: bool) {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        segments.push(Segment {
            text: trimmed.to_string(),
            atomic,
        });
    }
}

fn estimate_tokens(text: &str) -> usize {
    text.len() / 4 + 1
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            sentences.push(current.clone());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current);
    }
    sentences
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_no_chunks() {
        assert!(chunk_text("", &ChunkConfig::default()).is_empty());
        assert!(chunk_text("   ", &ChunkConfig::default()).is_empty());
    }

    #[test]
    fn short_text_single_chunk() {
        let chunks = chunk_text("Hello world.", &ChunkConfig::default());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].index, 0);
    }

    #[test]
    fn paragraph_splitting() {
        let text = "Paragraph one.\n\nParagraph two.\n\nParagraph three.";
        let config = ChunkConfig {
            chunk_size: 10,
            chunk_overlap: 2,
            parent_chunk_size: None,
        };
        let chunks = chunk_text(text, &config);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn chunk_indexes_sequential() {
        let text = "A. B. C.\n\nD. E. F.\n\nG. H. I.";
        let config = ChunkConfig {
            chunk_size: 5,
            chunk_overlap: 1,
            parent_chunk_size: None,
        };
        let chunks = chunk_text(text, &config);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
        }
    }

    #[test]
    fn split_structural_marks_table_atomic() {
        let text = "Intro paragraph.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\nOutro.";
        let segments = split_structural(text);
        let table = segments
            .iter()
            .find(|s| s.text.contains("| A | B |"))
            .expect("a table segment");
        assert!(table.atomic, "table must be marked atomic");
        // All rows grouped into the one atomic segment.
        assert!(table.text.contains("| 1 | 2 |") && table.text.contains("| 3 | 4 |"));
    }

    #[test]
    fn split_structural_marks_fenced_code_atomic() {
        let text = "Text before.\n\n```rust\nfn x() {}\nlet y = 1;\n```\n\nText after.";
        let segments = split_structural(text);
        let code = segments
            .iter()
            .find(|s| s.text.contains("fn x()"))
            .expect("a code segment");
        assert!(code.atomic, "fenced code must be marked atomic");
        assert!(code.text.contains("let y = 1;"), "code lines grouped");
    }

    #[test]
    fn small_table_stays_whole_in_one_chunk() {
        // Budget comfortably holds the whole doc → the table must appear intact
        // (rows contiguous) rather than scattered.
        let text = "Lead-in sentence.\n\n| Region | Rev |\n|---|---|\n| EU | 10 |\n| US | 20 |\n\nTrailing sentence.";
        let chunks = chunk_text(text, &ChunkConfig::default());
        let table_intact = chunks
            .iter()
            .any(|c| c.text.contains("| Region | Rev |") && c.text.contains("| US | 20 |"));
        assert!(table_intact, "table rows must stay together: {chunks:?}");
    }

    // --- attach_parent_windows / expand_window: P2-2 parent-child chunking.

    #[test]
    fn disabled_by_default_every_parent_text_is_none() {
        let text = "Paragraph one.\n\nParagraph two.\n\nParagraph three.";
        let config = ChunkConfig {
            chunk_size: 5,
            chunk_overlap: 1,
            parent_chunk_size: None,
        };
        let chunks = chunk_text(text, &config);
        assert!(chunks.len() > 1, "need multiple chunks to test with");
        assert!(chunks.iter().all(|c| c.parent_text.is_none()));
    }

    #[test]
    fn a_single_chunk_document_has_no_neighbors_to_borrow() {
        let config = ChunkConfig {
            parent_chunk_size: Some(1000),
            ..ChunkConfig::default()
        };
        let chunks = chunk_text("Just one short sentence.", &config);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].parent_text, None);
    }

    #[test]
    fn parent_window_includes_the_childs_own_text() {
        let config = ChunkConfig {
            chunk_size: 5,
            chunk_overlap: 1,
            parent_chunk_size: Some(5), // tight budget: barely fits the child alone
        };
        let text = "Paragraph one.\n\nParagraph two.\n\nParagraph three.";
        let chunks = chunk_text(text, &config);
        assert!(chunks.len() > 1, "need multiple chunks to test with");
        for chunk in &chunks {
            let parent = chunk.parent_text.as_ref().expect("parent_text set");
            assert!(
                parent.contains(chunk.text.as_str()),
                "parent window must contain its own child: {parent:?} vs {:?}",
                chunk.text
            );
        }
    }

    #[test]
    fn a_generous_budget_pulls_in_every_neighbor() {
        let texts = vec!["alpha", "beta", "gamma", "delta"];
        let window = expand_window(&texts, 1, 1000);
        assert_eq!(window, "alpha beta gamma delta");
    }

    #[test]
    fn a_tight_budget_returns_only_the_center_chunk() {
        let texts = vec!["a-longer-first-chunk", "beta", "a-longer-third-chunk"];
        // Center "beta" alone costs 2 tokens (4 chars); budget 3 leaves room for
        // only 1 more, but either 21-char neighbor costs ~6 — too big to fit.
        let window = expand_window(&texts, 1, 3);
        assert_eq!(window, "beta");
    }

    #[test]
    fn window_expands_symmetrically_when_budget_allows_one_neighbor_at_a_time() {
        // Each of "aa"/"bb"/"cc"/"dd" is 2 chars => 1 token. Center "cc" (index
        // 2) costs 1; budget 3 fits exactly two more one-token neighbors,
        // alternating left-then-right from the center.
        let texts = vec!["aa", "bb", "cc", "dd", "ee"];
        let window = expand_window(&texts, 2, 3);
        assert_eq!(window, "bb cc dd");
    }
}
