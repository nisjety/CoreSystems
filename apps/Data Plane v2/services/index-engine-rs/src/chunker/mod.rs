/// Token-aware text chunker.
/// Port of Python chunker from v1, using char-based estimation (4 chars ≈ 1 token for cl100k_base).
///
/// Strategy:
///   1. Split by paragraph boundaries (double newlines)
///   2. If paragraph exceeds chunk_size tokens, split by sentences
///   3. Greedily pack segments into chunks respecting token budget
///   4. Overlap is in estimated tokens

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

pub fn chunk_text(text: &str, config: &ChunkConfig) -> Vec<Chunk> {
    if text.trim().is_empty() {
        return vec![];
    }

    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    // Flatten large paragraphs into sentences
    let mut segments: Vec<String> = Vec::new();
    for para in &paragraphs {
        if estimate_tokens(para) <= config.chunk_size {
            segments.push(para.to_string());
        } else {
            for sentence in split_sentences(para) {
                let trimmed = sentence.trim();
                if !trimmed.is_empty() {
                    segments.push(trimmed.to_string());
                }
            }
        }
    }

    // Greedily pack segments into chunks with overlap
    let mut chunks: Vec<String> = Vec::new();
    let mut current_segments: Vec<String> = Vec::new();
    let mut current_tokens: usize = 0;

    for segment in &segments {
        let seg_tokens = estimate_tokens(segment);

        // Single segment exceeds chunk_size → window-split it
        if seg_tokens > config.chunk_size {
            if !current_segments.is_empty() {
                chunks.push(current_segments.join(" "));
                current_segments.clear();
                current_tokens = 0;
            }

            let step = config
                .chunk_size
                .saturating_sub(config.chunk_overlap)
                .max(1);
            let chars: Vec<char> = segment.chars().collect();
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

        if current_tokens + seg_tokens > config.chunk_size && !current_segments.is_empty() {
            chunks.push(current_segments.join(" "));

            // Keep trailing segments for overlap
            let mut overlap_segments: Vec<String> = Vec::new();
            let mut overlap_tokens: usize = 0;
            for seg in current_segments.iter().rev() {
                let seg_t = estimate_tokens(seg);
                if overlap_tokens + seg_t > config.chunk_overlap {
                    break;
                }
                overlap_segments.insert(0, seg.clone());
                overlap_tokens += seg_t;
            }

            current_segments = overlap_segments;
            current_segments.push(segment.clone());
            current_tokens = current_segments.iter().map(|s| estimate_tokens(s)).sum();
        } else {
            current_segments.push(segment.clone());
            current_tokens += seg_tokens;
        }
    }

    if !current_segments.is_empty() {
        chunks.push(current_segments.join(" "));
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
}
