//! REST resource envelope. Mirrors CONTRACTS §2.

use serde::{Deserialize, Serialize};

use crate::error::QuarryError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub data: Option<T>,
    pub meta: EnvelopeMeta,
    pub error: Option<QuarryError>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvelopeMeta {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    pub limit: u32,
}

impl<T> Envelope<T> {
    pub fn ok(request_id: impl Into<String>, data: T) -> Self {
        Self {
            data: Some(data),
            meta: EnvelopeMeta {
                request_id: request_id.into(),
                page: None,
            },
            error: None,
        }
    }

    pub fn err(request_id: impl Into<String>, err: QuarryError) -> Self {
        Self {
            data: None,
            meta: EnvelopeMeta {
                request_id: request_id.into(),
                page: None,
            },
            error: Some(err),
        }
    }
}
