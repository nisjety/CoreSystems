//! Recursive, structure-aware text chunker.
//!
//! Strategy (PR-E, phased — structural pass; DI-layout + Contextual Retrieval are
//! wired behind flags in config and remain off until a Document Intelligence
//! resource + per-chunk LLM budget are approved):
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

#[derive(Debug, Clone)]
pub struct ChunkConfig {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            chunk_overlap: 64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: usize,
    pub text: String,
    pub estimated_tokens: usize,
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

    chunks
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let estimated_tokens = estimate_tokens(&text);
            Chunk {
                index,
                text,
                estimated_tokens,
            }
        })
        .collect()
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
}
