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
                let take = (len - local).min(max_chars);
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
