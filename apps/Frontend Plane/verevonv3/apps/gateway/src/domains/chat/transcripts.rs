//! `POST /api/v1/chat/transcripts`: transcribe one audio upload and save the text as a Data Plane document (W5, F-t3-090).
//!
//! The recording goes to Model Gateway's speech route as an `stt` call, through `ai::proxy_speech`, so the caller's model
//! and inference bearers, the organisation's posture and the speech route's own checks are the ones a dictation meets.
//! The text is then saved through the leg a chat attachment uses: a private `media_transcript` document whose
//! `metadata.origin` is `chat-transcript`. The audio is never stored and never returned. The browser gets the document's
//! id, title and character count, and nothing else.
//!
//! Off while `CHAT_MEDIA_TRANSCRIPTS_ENABLED` is not the literal `true`: 404 `media_transcripts_disabled`. A Zero Data
//! Retention caller is refused with 412 `zdr_durable_mutation_forbidden` before the recording leaves the gateway, because
//! a transcript document is durable content. An organisation whose posture cannot be read is refused with 503
//! `zdr_decision_unavailable` (retryable), the answer every durable write of chat content gives.

use axum::{
    body::Bytes,
    extract::{Extension, State},
    http::{header::CACHE_CONTROL, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{config::AppState, domains::ai, middleware::AuthenticatedUser};

use super::documents::{text_field, CHAT_TRANSCRIPT_ORIGIN, CHAT_TRANSCRIPT_TYPE};

/// The longest title a transcript document carries, in characters.
const MAX_TITLE_CHARS: usize = 120;
/// The longest thread id the transcript's metadata may name. Ids are opaque labels, as a `source_ref` id is.
const MAX_THREAD_ID_CHARS: usize = 256;
/// The most of the speech route's answer that is read. The answer is the transcript text, which documents-api caps at
/// 5 MiB anyway.
const SPEECH_REPLY_LIMIT_BYTES: usize = 5 * 1024 * 1024;
/// The audio types this route takes, by base type. A codec parameter (`audio/webm;codecs=opus`) is ignored.
const ACCEPTED_AUDIO_TYPES: [&str; 5] = [
    "audio/mpeg",
    "audio/wav",
    "audio/webm",
    "audio/mp4",
    "audio/ogg",
];

/// The base type and the speech format name of an accepted audio type. The format names are the ones the dictation
/// route sends, so the speech route reads a transcript's recording exactly as it reads a dictation.
fn accepted_audio(mime: &str) -> Option<(&'static str, &'static str)> {
    let base = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match base.as_str() {
        "audio/mpeg" => Some(("audio/mpeg", "mp3")),
        "audio/wav" => Some(("audio/wav", "wav")),
        "audio/webm" => Some(("audio/webm", "webm")),
        // Safari records AAC in an mp4 container; the speech providers take it as m4a.
        "audio/mp4" => Some(("audio/mp4", "m4a")),
        "audio/ogg" => Some(("audio/ogg", "ogg")),
        _ => None,
    }
}

/// POST /api/v1/chat/transcripts: the body is `{audio_base64, mime, title, thread_id?}`. See the module doc.
pub(crate) async fn create_chat_transcript(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !state.space_features.chat_media_transcripts {
        return (
            StatusCode::NOT_FOUND,
            Json(crate::envelope::error(
                "media_transcripts_disabled",
                "Transcribing recordings is not available.",
            )),
        )
            .into_response();
    }
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return invalid_body(
            "Expected a JSON object with `audio_base64`, `mime`, `title` and an optional `thread_id`.",
        );
    };

    // Refused before anything is sent: the request's own flag (the header, or a body `zdr`) and then the organisation's
    // standing posture, decided by the gate every durable write of chat content passes. A posture that cannot be read is
    // the typed, retryable 503 that gate answers, never a guess either way.
    if let Some(refusal) =
        crate::zdr::durable_write_refusal(&state, &user, Some(&headers), Some(&payload)).await
    {
        return refusal.into_response();
    }

    let Some(title) =
        text_field(&payload, "title").filter(|title| title.chars().count() <= MAX_TITLE_CHARS)
    else {
        return invalid_body("`title` is required and must be at most 120 characters.");
    };
    let Some(audio_base64) = text_field(&payload, "audio_base64") else {
        return invalid_body("`audio_base64` is required.");
    };
    let Some((mime, format)) = text_field(&payload, "mime")
        .as_deref()
        .and_then(accepted_audio)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(crate::envelope::error_with_details(
                "unsupported_audio_type",
                "`mime` must be an audio type this route takes.",
                json!({ "allowed": ACCEPTED_AUDIO_TYPES }),
            )),
        )
            .into_response();
    };
    let thread_id = match payload.get("thread_id") {
        None | Some(Value::Null) => None,
        Some(_) => match text_field(&payload, "thread_id") {
            Some(thread_id) if thread_id.chars().count() <= MAX_THREAD_ID_CHARS => Some(thread_id),
            _ => {
                return invalid_body(
                    "`thread_id` must be a non-empty id of at most 256 characters.",
                )
            }
        },
    };

    // The speech route, exactly as `ai::proxy_speech` forwards an `stt` call: the bearers, the org posture and its
    // checks all apply here. Its answer is read back; the recording itself is not kept.
    let speech = ai::proxy_speech(
        State(state.clone()),
        Extension(user.clone()),
        headers.clone(),
        Json(json!({ "operation": "stt", "audio_base64": audio_base64, "format": format })),
    )
    .await
    .into_response();
    let (parts, reply_body) = speech.into_parts();
    let reply = axum::body::to_bytes(reply_body, SPEECH_REPLY_LIMIT_BYTES)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    if !parts.status.is_success() {
        return speech_failure(parts.status, &reply);
    }
    let Some(transcript) = reply.get("text").and_then(Value::as_str) else {
        return transcription_unavailable();
    };
    let transcript = transcript.trim();
    if transcript.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(crate::envelope::error(
                "transcript_empty",
                "No speech was recognised in this recording.",
            )),
        )
            .into_response();
    }

    let mut metadata = json!({
        "origin": CHAT_TRANSCRIPT_ORIGIN,
        "source_mime": mime,
    });
    if let Some(thread_id) = &thread_id {
        metadata["thread_id"] = json!(thread_id);
    }
    // The speech route reports a duration only when it has one; a transcript that has none carries no key.
    if let Some(duration) = reply
        .get("duration")
        .filter(|duration| duration.is_number())
    {
        metadata["duration"] = duration.clone();
    }
    let chars = transcript.chars().count();
    let org_id = {
        let resolved = crate::upstream::authorized_org_id(&state, &user).await;
        (!resolved.is_empty()).then_some(resolved)
    };
    // The owner is the verified principal that documents-api pins on a create, so the body names no owner. `private`
    // is stated, not left to the default.
    let document = json!({
        "org_id": org_id.clone().unwrap_or_default(),
        // The source label a chat attachment carries: the user's own upload, transcribed.
        "source": "chat-upload",
        "type": CHAT_TRANSCRIPT_TYPE,
        "title": title,
        "content": transcript,
        "zdr_classification": "internal",
        "visibility": "private",
        "metadata": metadata,
    });
    let url = format!("{}/v1/documents", state.documents_api_url);
    let (saved_status, Json(saved)) = crate::domains::knowledge::shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(document),
        org_id.as_deref(),
        None,
    )
    .await;
    let document_id = saved
        .get("document_id")
        .and_then(Value::as_str)
        .filter(|document_id| !document_id.is_empty() && saved_status.is_success());
    let Some(document_id) = document_id else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(crate::envelope::error(
                "transcript_not_saved",
                "The transcript could not be saved as a document.",
            )),
        )
            .into_response();
    };

    let mut response = (
        StatusCode::CREATED,
        Json(crate::envelope::ok(json!({
            "document_id": document_id,
            "title": title,
            "chars": chars,
        }))),
    )
        .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn invalid_body(message: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(crate::envelope::error("invalid_body", message)),
    )
        .into_response()
}

/// The Zero Data Retention refusal of a transcript, in the catalogue's shape. A transcript document is durable content,
/// so the catalogue's `zdr_durable_mutation_forbidden` (not retryable, no action) is the answer.
fn zdr_refusal() -> Response {
    (
        StatusCode::PRECONDITION_FAILED,
        Json(crate::zdr::zdr_refusal_body(
            crate::zdr::ZDR_DURABLE_MUTATION_FORBIDDEN,
        )),
    )
        .into_response()
}

fn transcription_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(crate::envelope::error(
            "transcription_unavailable",
            "Transcription is temporarily unavailable.",
        )),
    )
        .into_response()
}

/// A refusal from the speech route, mapped to this route's answers. A recording the speech route will not take is the
/// caller's to change. Only a typed Zero Data Retention refusal (an `error.code` that starts `zdr_`) is this route's
/// refusal too: Model Gateway answers every gRPC `FailedPrecondition` with 412, so the status alone does not say ZDR.
/// Anything else, including a delegated credential the speech route could not use, is a temporary failure.
fn speech_failure(status: StatusCode, reply: &Value) -> Response {
    let typed_zdr = reply
        .pointer("/error/code")
        .and_then(Value::as_str)
        .is_some_and(|code| code.starts_with("zdr_"));
    match status {
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(crate::envelope::error(
                "transcription_failed",
                "The recording could not be transcribed.",
            )),
        )
            .into_response(),
        StatusCode::PRECONDITION_FAILED if typed_zdr => zdr_refusal(),
        _ => transcription_unavailable(),
    }
}
