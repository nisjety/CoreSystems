//! Response-body decoding for the ERP REST client.

use std::io::Read;

use flate2::read::MultiGzDecoder;
use vsv2_common::{Error, Result};

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Most a compressed body may inflate to. The background API delivers whole tables, so this is
/// generous; it exists so a corrupt or hostile stream cannot exhaust memory.
const MAX_INFLATED_BYTES: u64 = 1 << 30;

/// The body of a successful response as text.
///
/// reqwest inflates a response that declares `Content-Encoding: gzip`. A body that still starts
/// with the gzip magic number after that arrived compressed without saying so (seen on the
/// background API's `/content` download, where `.text()` returned the compressed bytes and JSON
/// parsing failed on them), so it is inflated here instead of failing the whole endpoint.
pub(crate) fn body_text(bytes: &[u8]) -> Result<String> {
    if !bytes.starts_with(&GZIP_MAGIC) {
        return Ok(String::from_utf8_lossy(bytes).into_owned());
    }
    let mut inflated = Vec::with_capacity(bytes.len().saturating_mul(4));
    MultiGzDecoder::new(bytes)
        .take(MAX_INFLATED_BYTES + 1)
        .read_to_end(&mut inflated)
        .map_err(|e| Error::Http(format!("inflate erp_rest body: {e}")))?;
    if inflated.len() as u64 > MAX_INFLATED_BYTES {
        return Err(Error::Http(format!(
            "inflate erp_rest body: more than {MAX_INFLATED_BYTES} bytes"
        )));
    }
    Ok(match String::from_utf8(inflated) {
        Ok(text) => text,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    })
}

/// The body of an error response as text: inflated when possible, otherwise lossy, and never
/// an error itself, so a truncated body cannot hide the status it came with.
pub(crate) fn error_text(bytes: &[u8]) -> String {
    body_text(bytes).unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{write::GzEncoder, Compression};

    use super::*;

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn plain_json_passes_through() {
        assert_eq!(body_text(br#"[{"a":1}]"#).unwrap(), r#"[{"a":1}]"#);
    }

    #[test]
    fn compressed_body_is_inflated() {
        let json = r#"[{"internalId":1},{"internalId":2}]"#;
        assert_eq!(body_text(&gzip(json.as_bytes())).unwrap(), json);
    }

    #[test]
    fn large_compressed_body_is_inflated() {
        let json = format!("[{}]", vec![r#"{"n":"abcdefghij"}"#; 50_000].join(","));
        assert_eq!(body_text(&gzip(json.as_bytes())).unwrap(), json);
    }

    #[test]
    fn concatenated_gzip_members_are_all_read() {
        let mut bytes = gzip(b"[1,2,");
        bytes.extend(gzip(b"3,4]"));
        assert_eq!(body_text(&bytes).unwrap(), "[1,2,3,4]");
    }

    #[test]
    fn corrupt_compressed_body_is_an_error_not_garbage() {
        let mut bytes = gzip(b"[1,2,3]");
        bytes.truncate(bytes.len() / 2);
        let err = body_text(&bytes).unwrap_err().to_string();
        assert!(err.contains("inflate erp_rest body"), "{err}");
    }

    #[test]
    fn a_truncated_error_body_still_yields_text() {
        let mut bytes = gzip(b"upstream exploded");
        bytes.truncate(bytes.len() / 2);
        assert!(!error_text(&bytes).is_empty());
    }

    #[test]
    fn empty_body_is_empty_text() {
        assert_eq!(body_text(b"").unwrap(), "");
    }
}
