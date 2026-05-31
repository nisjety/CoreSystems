//! Opaque pagination cursor with canonical encoding.

use serde::{Deserialize, Serialize};

use crate::error::{EventLogError, EventLogResult};

/// The decoded payload inside a cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorPayload {
    /// Offset into the event log (event sequence number).
    pub offset: u64,
}

/// An opaque, base64-encoded pagination cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    payload: CursorPayload,
}

impl Cursor {
    /// Construct a cursor from a payload.
    #[must_use]
    pub fn new(payload: CursorPayload) -> Self {
        Self { payload }
    }

    /// Encode the cursor to a base64 string for transmission.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn encode(&self) -> EventLogResult<String> {
        let bytes = serde_json::to_vec(&self.payload)
            .map_err(|e| EventLogError::CursorDecode(e.to_string()))?;
        Ok(base64_encode(&bytes))
    }

    /// Decode a cursor from a base64 string.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is not valid base64 or the payload is malformed.
    pub fn decode(encoded: &str) -> EventLogResult<Self> {
        let bytes = base64_decode(encoded).map_err(EventLogError::CursorDecode)?;
        let payload: CursorPayload = serde_json::from_slice(&bytes)
            .map_err(|e| EventLogError::CursorDecode(e.to_string()))?;
        Ok(Self { payload })
    }

    /// Return the decoded payload.
    #[must_use]
    pub fn payload(&self) -> &CursorPayload {
        &self.payload
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() * 4 / 3) + 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };
        let _ = write!(out, "{}", TABLE[(b0 >> 2) & 0x3F] as char);
        let _ = write!(out, "{}", TABLE[((b0 << 4) | (b1 >> 4)) & 0x3F] as char);
        let _ = write!(
            out,
            "{}",
            if chunk.len() > 1 {
                TABLE[((b1 << 2) | (b2 >> 6)) & 0x3F] as char
            } else {
                '='
            }
        );
        let _ = write!(
            out,
            "{}",
            if chunk.len() > 2 {
                TABLE[b2 & 0x3F] as char
            } else {
                '='
            }
        );
    }
    out
}

fn base64_decode(encoded: &str) -> Result<Vec<u8>, String> {
    // Minimal decoder — delegates to standard library via a simple approach.
    let encoded = encoded.trim_end_matches('=');
    let mut out = Vec::with_capacity(encoded.len() * 3 / 4);
    let table: [u8; 128] = {
        let mut t = [255u8; 128];
        for (i, &c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
            .iter()
            .enumerate()
        {
            t[c as usize] = u8::try_from(i).unwrap_or(255);
        }
        t
    };
    let bytes: Vec<u8> = encoded.bytes().collect();
    for chunk in bytes.chunks(4) {
        let get = |i: usize| -> Result<u8, String> {
            let b = *chunk.get(i).unwrap_or(&b'A') as usize;
            if b >= 128 || table[b] == 255 {
                Err(format!("invalid base64 character at position {i}"))
            } else {
                Ok(table[b])
            }
        };
        let b0 = get(0)?;
        let b1 = get(1)?;
        out.push((b0 << 2) | (b1 >> 4));
        if chunk.len() > 2 {
            let b2 = get(2)?;
            out.push(((b1 & 0x0F) << 4) | (b2 >> 2));
            if chunk.len() > 3 {
                let b3 = get(3)?;
                out.push(((b2 & 0x03) << 6) | b3);
            }
        }
    }
    Ok(out)
}
