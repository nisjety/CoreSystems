//! Vision input for chat (chat-parity §2 multimodal).
//!
//! When a chat request carries an image attachment, the gateway routes it to
//! inference-core's `AnalyzeImage` RPC (the canonical vision owner) with the
//! user's message as the prompt, and streams the analysis back as the answer.
//! No vision model is embedded in the gateway.

use base64::Engine as _;
use serde::Deserialize;

/// An attachment on a chat request. Images are routed to vision; other kinds
/// are ignored here (handled by file-upload/RAG paths).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct InvokeAttachment {
    /// "image" | "document" | … (defaults to inferring from `mime_type`).
    #[serde(default)]
    pub kind: Option<String>,
    /// Public image URL (mutually exclusive with `data_base64`).
    #[serde(default)]
    pub url: Option<String>,
    /// Inline base64-encoded image bytes.
    #[serde(default)]
    pub data_base64: Option<String>,
    /// MIME type, e.g. "image/png".
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// A resolved image ready for `AnalyzeImageRequest` (exactly one of url/data is
/// populated; bytes win when both are present).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInput {
    pub url: String,
    pub data: Vec<u8>,
    pub mime_type: String,
}

fn is_image(att: &InvokeAttachment) -> bool {
    match att.kind.as_deref() {
        Some(k) => k.eq_ignore_ascii_case("image"),
        None => att
            .mime_type
            .as_deref()
            .is_some_and(|m| m.starts_with("image/")),
    }
}

/// Pick the first image attachment and resolve it to an [`ImageInput`].
/// Returns `None` when there is no image or its data is unusable (bad base64 /
/// no url) — the caller then proceeds as a normal text chat.
#[must_use]
pub fn select_image(attachments: &[InvokeAttachment]) -> Option<ImageInput> {
    let att = attachments.iter().find(|a| is_image(a))?;
    let mime_type = att
        .mime_type
        .clone()
        .unwrap_or_else(|| "image/png".to_owned());

    if let Some(b64) = att.data_base64.as_deref().filter(|s| !s.is_empty()) {
        // Tolerate a `data:` URL prefix ("data:image/png;base64,….").
        let payload = b64.rsplit(',').next().unwrap_or(b64);
        match base64::engine::general_purpose::STANDARD.decode(payload) {
            Ok(bytes) if !bytes.is_empty() => {
                return Some(ImageInput {
                    url: String::new(),
                    data: bytes,
                    mime_type,
                });
            }
            _ => return None,
        }
    }

    let url = att.url.as_deref().filter(|s| !s.is_empty())?;
    Some(ImageInput {
        url: url.to_owned(),
        data: Vec::new(),
        mime_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img_url(url: &str) -> InvokeAttachment {
        InvokeAttachment {
            kind: Some("image".to_owned()),
            url: Some(url.to_owned()),
            mime_type: Some("image/png".to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn selects_image_by_url() {
        let got = select_image(&[img_url("https://x/y.png")]).unwrap();
        assert_eq!(got.url, "https://x/y.png");
        assert!(got.data.is_empty());
        assert_eq!(got.mime_type, "image/png");
    }

    #[test]
    fn decodes_inline_base64_and_strips_data_url_prefix() {
        // base64 of "hi" = "aGk="
        let att = InvokeAttachment {
            kind: Some("image".to_owned()),
            data_base64: Some("data:image/png;base64,aGk=".to_owned()),
            mime_type: Some("image/png".to_owned()),
            ..Default::default()
        };
        let got = select_image(&[att]).unwrap();
        assert_eq!(got.data, b"hi");
        assert!(got.url.is_empty());
    }

    #[test]
    fn infers_image_from_mime_when_kind_absent() {
        let att = InvokeAttachment {
            url: Some("https://x/y.jpg".to_owned()),
            mime_type: Some("image/jpeg".to_owned()),
            ..Default::default()
        };
        assert!(select_image(&[att]).is_some());
    }

    #[test]
    fn ignores_non_image_and_empty() {
        let doc = InvokeAttachment {
            kind: Some("document".to_owned()),
            url: Some("https://x/y.pdf".to_owned()),
            mime_type: Some("application/pdf".to_owned()),
            ..Default::default()
        };
        assert!(select_image(&[doc]).is_none());
        assert!(select_image(&[]).is_none());
    }

    #[test]
    fn rejects_bad_base64() {
        let att = InvokeAttachment {
            kind: Some("image".to_owned()),
            data_base64: Some("!!!not base64!!!".to_owned()),
            ..Default::default()
        };
        assert!(select_image(&[att]).is_none());
    }
}
