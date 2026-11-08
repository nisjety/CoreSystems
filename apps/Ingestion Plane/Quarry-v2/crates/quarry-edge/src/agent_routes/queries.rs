//! The query-string types of the live-frame and event-stream routes and the lenient number
//! parsing they share (a browser sends `?quality=` as text). Split from `types.rs` to keep it
//! near the 650-line target (WS2.B).

use super::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameQuery {
    /// An owner-authorized, read-only view of this session's specific tab.
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_query_u8")]
    pub quality: Option<u8>,
    #[serde(default, deserialize_with = "deserialize_optional_query_u32")]
    pub max_width: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_optional_query_u32")]
    pub max_height: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_optional_query_u32")]
    pub every_nth_frame: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameStreamQuery {
    #[serde(flatten)]
    pub frame: FrameQuery,
    #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
    pub interval_ms: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
    pub max_frames: Option<u64>,
}

/// W3 — query for `GET /v1/agent/runs/{id}/events/stream`.
/// `from_seq` replays the run's in-memory trace ring after that seq before the
/// live tail (WS10.S3; the `Last-Event-ID` header is the fallback); `max_events`
/// caps the total emission count for the connection.
#[derive(Debug, Deserialize)]
pub struct AgentEventsQuery {
    #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
    pub from_seq: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
    pub max_events: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum QueryScalar<T> {
    Number(T),
    String(String),
}

pub(super) fn deserialize_optional_query_u8<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_query_number(deserializer)
}

pub(super) fn deserialize_optional_query_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_query_number(deserializer)
}

pub(super) fn deserialize_optional_query_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_query_number(deserializer)
}

pub(super) fn deserialize_optional_query_number<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + FromStr,
    T::Err: Display,
{
    match Option::<QueryScalar<T>>::deserialize(deserializer)? {
        Some(QueryScalar::Number(value)) => Ok(Some(value)),
        Some(QueryScalar::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                trimmed.parse::<T>().map(Some).map_err(D::Error::custom)
            }
        }
        None => Ok(None),
    }
}
