/// BLAKE3-based content fingerprinting for stable chunk IDs and deduplication.
pub fn content_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

/// Generate a stable chunk ID from document_id + chunk_index + content_hash.
/// Same document with same content at same position = same ID across reruns.
pub fn stable_chunk_id(document_id: &str, chunk_index: usize, content_hash: &str) -> String {
    let input = format!("{document_id}:{chunk_index}:{content_hash}");
    let hash = blake3::hash(input.as_bytes());
    // Use first 32 hex chars as a UUID-like identifier
    let hex = hash.to_hex();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_hash() {
        let h1 = content_hash("hello world");
        let h2 = content_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_content_different_hash() {
        let h1 = content_hash("hello");
        let h2 = content_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn stable_id_deterministic() {
        let id1 = stable_chunk_id("doc-1", 0, "abc123");
        let id2 = stable_chunk_id("doc-1", 0, "abc123");
        assert_eq!(id1, id2);
    }

    #[test]
    fn stable_id_different_index() {
        let id1 = stable_chunk_id("doc-1", 0, "abc123");
        let id2 = stable_chunk_id("doc-1", 1, "abc123");
        assert_ne!(id1, id2);
    }
}
