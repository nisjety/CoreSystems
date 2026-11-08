//! Chunk boundaries for downstream consumers (retrieval lives in Data Plane).
//! Quarry only *emits* chunk offsets; never runs embeddings.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

pub fn paragraph_chunks(text: &str, max_chars: usize) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for para in text.split("\n\n") {
        let start = cursor;
        let len = para.len();
        if len == 0 {
            cursor += 2;
            continue;
        }
        if len <= max_chars {
            out.push(Chunk {
                start,
                end: start + len,
                text: para.to_string(),
            });
        } else {
            let mut local = 0;
            while local < len {
                // Cut on a character boundary: a paragraph with multi-byte characters would
                // otherwise panic on the slice. A cut never moves forward past the paragraph, and
                // when `max_chars` is smaller than the next character the chunk is that character.
                let mut take = (len - local).min(max_chars);
                while take > 0 && !para.is_char_boundary(local + take) {
                    take -= 1;
                }
                if take == 0 {
                    take = para[local..]
                        .chars()
                        .next()
                        .map_or(len - local, char::len_utf8);
                }
                let slice = &para[local..local + take];
                out.push(Chunk {
                    start: start + local,
                    end: start + local + take,
                    text: slice.to_string(),
                });
                local += take;
            }
        }
        cursor += len + 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_paragraph_of_multibyte_text_is_cut_on_character_boundaries() {
        // 3-byte characters, max_chars not a multiple of 3: a byte cut would panic.
        let text = "\u{20ac}".repeat(50);
        let chunks = paragraph_chunks(&text, 10);
        assert!(chunks.len() > 1);
        assert_eq!(
            chunks.iter().map(|c| c.text.as_str()).collect::<String>(),
            text
        );
        assert!(chunks
            .iter()
            .all(|c| c.text.len() <= 10 && !c.text.is_empty()));
    }

    #[test]
    fn a_limit_smaller_than_one_character_still_makes_progress() {
        let chunks = paragraph_chunks("\u{20ac}\u{20ac}", 1);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "\u{20ac}");
    }

    #[test]
    fn ascii_paragraphs_chunk_as_before() {
        let chunks = paragraph_chunks("abcdef\n\nxy", 4);
        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["abcd", "ef", "xy"]);
    }
}
