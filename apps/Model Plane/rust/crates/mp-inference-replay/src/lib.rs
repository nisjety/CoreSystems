//! Replay of recorded model sessions for the model-gateway and execution-core suites.
//!
//! A cassette is one synthetic model session in `mp-replay-v1` JSONL: a header line, then one
//! line per model call. [`ReplayInference`] serves those calls over the real `InferenceCore`
//! contract, so a test runs the actual prompt assembly, tool parsing and loop control against it
//! and asserts on the result. Matching is structural: the n-th request must carry the call's
//! purpose, offer the tools the cassette expects and end on the role it expects. Text is never
//! compared, so a prompt edit does not break a cassette, while a change in what the model is
//! offered or asked for does.
//!
//! Dev-only (see `Cargo.toml`). `tests/cassette_lint.rs` keeps the shipped cassettes synthetic and
//! not ZDR, and keeps this crate out of every service's `[dependencies]`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures::Stream;
use mp_contracts::model_plane::v1::{
    self as pb,
    inference_core_server::{InferenceCore, InferenceCoreServer},
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Endpoint, Server};
use tonic::{Request, Response, Status};

/// The schema tag every cassette header carries.
pub const SCHEMA: &str = "mp-replay-v1";

/// The provider a replayed answer reports. A fixture name, never a real deployment.
const PROVIDER: &str = "replay-fixture";

/// The directory of the shipped cassettes.
pub fn cassette_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("cassettes")
}

/// Why a cassette cannot be loaded or served.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    /// Only fabricated sessions may be replayed; a recorded one would carry real user content.
    #[error("cassette `{0}` is not marked synthetic; only synthetic sessions may be replayed")]
    NotSynthetic(String),
    /// Zero-retention sessions are never recorded, so none may be checked in.
    #[error("cassette `{0}` is marked zdr; zero-retention sessions are never recorded")]
    Zdr(String),
    /// Anything else that makes the file unreadable or inconsistent, with the reason.
    #[error("cassette `{id}`: {reason}")]
    Invalid { id: String, reason: String },
}

fn invalid(id: &str, reason: impl Into<String>) -> ReplayError {
    ReplayError::Invalid {
        id: id.to_owned(),
        reason: reason.into(),
    }
}

/// The first line of a cassette.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub schema: String,
    pub id: String,
    pub synthetic: bool,
    pub zdr: bool,
    pub description: String,
    #[serde(default)]
    pub tokenised_ids: Vec<TokenSpec>,
}

/// A volatile identifier the cassette writes as `{{id:N}}`. Every replay binds it to a fresh value
/// that starts with `prefix`, so no id from one session can leak into another.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenSpec {
    pub id: u32,
    pub prefix: String,
}

/// One recorded model call, in the order the session makes them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub call: u32,
    /// The call's purpose, named as `InferPurpose` names it without its `INFER_PURPOSE_` prefix,
    /// for example `ANSWER`. Matching compares it with `InferRequest.purpose`.
    pub purpose: String,
    pub expect: Expect,
    pub response: Reply,
}

/// What the request must look like for this call to be the right one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    #[serde(default)]
    pub tools_offered: Vec<String>,
    pub last_role: String,
}

/// What the model answers. A unary call returns `content` and `tool_calls`; a streamed call emits
/// `deltas` in order, or `content` as one delta when `deltas` is empty.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub deltas: Vec<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallSpec>,
    #[serde(default)]
    pub stop_reason: String,
    #[serde(default)]
    pub input_tokens: i32,
    #[serde(default)]
    pub output_tokens: i32,
}

/// A tool call the model proposes. `arguments` is a JSON object, written as such in the file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallSpec {
    pub id: String,
    pub name: String,
    #[serde(default = "empty_object")]
    pub arguments: Value,
}

fn empty_object() -> Value {
    json!({})
}

/// One parsed and validated cassette.
#[derive(Debug, Clone, PartialEq)]
pub struct Cassette {
    pub header: Header,
    pub calls: Vec<Call>,
}

impl Cassette {
    /// Parse and validate `mp-replay-v1` JSONL. The synthetic and ZDR checks read the raw header
    /// before the typed one, so a refused file is refused for that reason and not for a parse error.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayError::NotSynthetic`] or [`ReplayError::Zdr`] for a session that may not be
    /// replayed, and [`ReplayError::Invalid`] for any other malformed or inconsistent cassette.
    pub fn parse(source: &str) -> Result<Self, ReplayError> {
        let mut lines = source.lines().filter(|line| !line.trim().is_empty());
        let header_line = lines
            .next()
            .ok_or_else(|| invalid("<empty>", "no header line"))?;
        let raw: Value = serde_json::from_str(header_line)
            .map_err(|error| invalid("<unreadable>", format!("header is not JSON: {error}")))?;
        let id = raw
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("<unnamed>")
            .to_owned();
        if raw.get("synthetic") != Some(&Value::Bool(true)) {
            return Err(ReplayError::NotSynthetic(id));
        }
        if raw.get("zdr") == Some(&Value::Bool(true)) {
            return Err(ReplayError::Zdr(id));
        }
        let header: Header = serde_json::from_value(raw)
            .map_err(|error| invalid(&id, format!("header: {error}")))?;
        if header.schema != SCHEMA {
            return Err(invalid(
                &id,
                format!("schema is `{}`, expected `{SCHEMA}`", header.schema),
            ));
        }
        let declared = declared_tokens(&header)?;
        let body: Vec<&str> = lines.collect();
        let mut calls = Vec::with_capacity(body.len());
        let mut used = BTreeSet::new();
        for (index, line) in body.iter().enumerate() {
            let call: Call = serde_json::from_str(line).map_err(|error| {
                invalid(&header.id, format!("call line {}: {error}", index + 1))
            })?;
            if usize::try_from(call.call).ok() != Some(index + 1) {
                return Err(invalid(
                    &header.id,
                    format!("call {} is out of order; expected {}", call.call, index + 1),
                ));
            }
            if purpose_value(&call.purpose).is_none() {
                return Err(invalid(
                    &header.id,
                    format!(
                        "call {} names purpose `{}`, which is not an InferPurpose",
                        call.call, call.purpose
                    ),
                ));
            }
            if !call.response.deltas.is_empty()
                && call.response.deltas.concat() != call.response.content
            {
                return Err(invalid(
                    &header.id,
                    format!("call {}: deltas do not add up to content", call.call),
                ));
            }
            used.extend(tokens_in(line, &header.id)?);
            calls.push(call);
        }
        if let Some(token) = used.difference(&declared).next() {
            return Err(invalid(
                &header.id,
                format!("token {{{{id:{token}}}}} is used but not declared"),
            ));
        }
        if let Some(token) = declared.difference(&used).next() {
            return Err(invalid(
                &header.id,
                format!("declared token {token} is never used"),
            ));
        }
        Ok(Self { header, calls })
    }
}

fn declared_tokens(header: &Header) -> Result<BTreeSet<u32>, ReplayError> {
    let mut declared = BTreeSet::new();
    for token in &header.tokenised_ids {
        if !declared.insert(token.id) {
            return Err(invalid(
                &header.id,
                format!("token {} is declared twice", token.id),
            ));
        }
    }
    Ok(declared)
}

/// The number of every `{{id:N}}` in `line`. A token opened without a number and a closing `}}` is
/// an error, so a typo cannot leave a literal `{{id:` in a served reply.
fn tokens_in(line: &str, id: &str) -> Result<BTreeSet<u32>, ReplayError> {
    const OPEN: &str = "{{id:";
    let mut found = BTreeSet::new();
    let mut rest = line;
    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at + OPEN.len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let (number, tail) = after.split_at(digits);
        match number.parse::<u32>() {
            Ok(token) if tail.starts_with("}}") => {
                found.insert(token);
                rest = &tail[2..];
            }
            _ => return Err(invalid(id, format!("malformed token after `{OPEN}`"))),
        }
    }
    Ok(found)
}

/// The `InferPurpose` value a cassette names, for example `ANSWER` for `INFER_PURPOSE_ANSWER`.
fn purpose_value(name: &str) -> Option<i32> {
    pb::InferPurpose::from_str_name(&format!("INFER_PURPOSE_{name}")).map(|purpose| purpose as i32)
}

/// The structure matching looks at, as canonical JSON: the purpose value, the offered tool names,
/// the role of the last message and, per message, its role, length and digest. The digests let a
/// failing diff name the message that moved without printing any of its text. The request id is
/// named too, because it says which part of the gateway made the call.
pub fn manifest(request: &pb::InferRequest) -> Value {
    json!({
        "request_id": request.request_id,
        "purpose": request.purpose,
        "tools_offered": sorted_names(request.tools.iter().map(|tool| tool.name.as_str())),
        "last_role": request.messages.last().map_or("", |message| message.role.as_str()),
        "messages": request.messages.iter().map(|message| json!({
            "role": message.role,
            "chars": message.content.chars().count(),
            "sha256": hex(&Sha256::digest(message.content.as_bytes())),
        })).collect::<Vec<_>>(),
    })
}

fn sorted_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut names: Vec<String> = names.map(str::to_owned).collect();
    names.sort();
    names
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// `text` with every `{{id:N}}` replaced by the value bound to N in this session.
fn rebind(text: &str, bindings: &BTreeMap<u32, String>) -> String {
    bindings
        .iter()
        .fold(text.to_owned(), |text, (token, value)| {
            text.replace(&format!("{{{{id:{token}}}}}"), value)
        })
}

/// A recorded reply with its volatile ids bound to the session.
struct Served {
    content: String,
    deltas: Vec<String>,
    tool_calls: Vec<pb::ToolCall>,
    stop_reason: String,
    input_tokens: i32,
    output_tokens: i32,
}

fn served_reply(reply: &Reply, bindings: &BTreeMap<u32, String>) -> Served {
    Served {
        content: rebind(&reply.content, bindings),
        deltas: reply
            .deltas
            .iter()
            .map(|delta| rebind(delta, bindings))
            .collect(),
        tool_calls: reply
            .tool_calls
            .iter()
            .map(|call| pb::ToolCall {
                id: rebind(&call.id, bindings),
                name: call.name.clone(),
                arguments_json: rebind(&call.arguments.to_string(), bindings),
            })
            .collect(),
        stop_reason: if reply.stop_reason.is_empty() {
            "end_turn".to_owned()
        } else {
            reply.stop_reason.clone()
        },
        input_tokens: reply.input_tokens,
        output_tokens: reply.output_tokens,
    }
}

fn model_used(requested: &str) -> String {
    if requested.is_empty() {
        "replay".to_owned()
    } else {
        requested.to_owned()
    }
}

fn unary(request: &pb::InferRequest, served: Served) -> pb::InferResponse {
    pb::InferResponse {
        request_id: request.request_id.clone(),
        content: served.content,
        model_used: model_used(&request.model),
        stop_reason: served.stop_reason,
        input_tokens: served.input_tokens,
        output_tokens: served.output_tokens,
        tool_calls: served.tool_calls,
        provider_used: PROVIDER.to_owned(),
        ..Default::default()
    }
}

fn streamed(request: &pb::InferRequest, mut served: Served) -> Vec<pb::InferChunk> {
    let pieces = if served.deltas.is_empty() {
        vec![served.content.clone()]
    } else {
        std::mem::take(&mut served.deltas)
    };
    let mut chunks: Vec<pb::InferChunk> = pieces
        .into_iter()
        .filter(|delta| !delta.is_empty())
        .map(|delta| pb::InferChunk {
            request_id: request.request_id.clone(),
            delta,
            ..Default::default()
        })
        .collect();
    chunks.push(pb::InferChunk {
        request_id: request.request_id.clone(),
        done: true,
        model_used: model_used(&request.model),
        input_tokens: served.input_tokens,
        output_tokens: served.output_tokens,
        provider_used: PROVIDER.to_owned(),
        stop_reason: served.stop_reason,
        ..Default::default()
    });
    chunks
}

/// The state of one replay: the cursor into the cassette, the ids bound to it and the first
/// mismatch, kept so [`ReplayInference::finish`] reports it even when the code under test swallows
/// the status the request received.
struct Session {
    cassette: Cassette,
    bindings: BTreeMap<u32, String>,
    served: usize,
    first_failure: Option<String>,
}

impl Session {
    fn refuse(&mut self, reason: String) -> Status {
        self.first_failure.get_or_insert_with(|| reason.clone());
        Status::failed_precondition(reason)
    }
}

/// A replayed model. Clones share one session, so the copy the gRPC server holds and the copy a
/// test keeps for [`ReplayInference::finish`] see the same cursor.
#[derive(Clone)]
pub struct ReplayInference {
    session: Arc<Mutex<Session>>,
}

impl ReplayInference {
    /// A fresh session of `cassette`, with each volatile id bound to a new value.
    pub fn new(cassette: Cassette) -> Self {
        let bindings: BTreeMap<u32, String> = cassette
            .header
            .tokenised_ids
            .iter()
            .map(|token| (token.id, format!("{}{}", token.prefix, ulid::Ulid::new())))
            .collect();
        Self {
            session: Arc::new(Mutex::new(Session {
                cassette,
                bindings,
                served: 0,
                first_failure: None,
            })),
        }
    }

    /// Calls served so far.
    pub fn served(&self) -> usize {
        self.lock().served
    }

    /// `Ok` when every request matched and every recorded call was served; otherwise the reason.
    ///
    /// # Errors
    ///
    /// Returns the first request that did not match, or the count of calls the session never reached.
    pub fn finish(&self) -> Result<(), String> {
        let session = self.lock();
        if let Some(failure) = &session.first_failure {
            return Err(failure.clone());
        }
        let total = session.cassette.calls.len();
        if session.served < total {
            return Err(format!(
                "cassette `{}` served {} of {total} calls: the scenario stopped before its last recorded model call",
                session.cassette.header.id, session.served
            ));
        }
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The reply for the next recorded call, or the refusal when `request` is not that call.
    fn reply_for(&self, request: &pb::InferRequest) -> Result<Served, Status> {
        let mut session = self.lock();
        let observed = manifest(request);
        let id = session.cassette.header.id.clone();
        let Some(call) = session.cassette.calls.get(session.served).cloned() else {
            let reason = format!(
                "cassette `{id}` has no call {} left\nobserved {}",
                session.served + 1,
                pretty(&observed)
            );
            return Err(session.refuse(reason));
        };
        let mut expected_tools = call.expect.tools_offered.clone();
        expected_tools.sort();
        let offered = sorted_names(request.tools.iter().map(|tool| tool.name.as_str()));
        let last_role = request
            .messages
            .last()
            .map_or("", |message| message.role.as_str());
        let purpose_matches = purpose_value(&call.purpose) == Some(request.purpose);
        if !purpose_matches || offered != expected_tools || last_role != call.expect.last_role {
            let reason = format!(
                "call {} of cassette `{id}` does not match the request\nexpected {}\nobserved {}",
                call.call,
                pretty(&json!({
                    "purpose": call.purpose,
                    "tools_offered": expected_tools,
                    "last_role": call.expect.last_role,
                })),
                pretty(&observed),
            );
            return Err(session.refuse(reason));
        }
        session.served += 1;
        Ok(served_reply(&call.response, &session.bindings))
    }
}

fn unserved(rpc: &str) -> Status {
    Status::unimplemented(format!(
        "{rpc} is not served by the replay fake, which replays infer and infer_stream"
    ))
}

type InferStream = Pin<Box<dyn Stream<Item = Result<pb::InferChunk, Status>> + Send>>;
type VideoStream =
    Pin<Box<dyn Stream<Item = Result<pb::StreamVideoGenerationContentResponse, Status>> + Send>>;

#[tonic::async_trait]
impl InferenceCore for ReplayInference {
    type InferStreamStream = InferStream;
    type StreamVideoGenerationContentStream = VideoStream;

    async fn infer(
        &self,
        request: Request<pb::InferRequest>,
    ) -> Result<Response<pb::InferResponse>, Status> {
        let request = request.into_inner();
        let served = self.reply_for(&request)?;
        Ok(Response::new(unary(&request, served)))
    }

    async fn infer_stream(
        &self,
        request: Request<pb::InferRequest>,
    ) -> Result<Response<Self::InferStreamStream>, Status> {
        let request = request.into_inner();
        let served = self.reply_for(&request)?;
        let chunks = streamed(&request, served);
        let stream = futures::stream::iter(chunks.into_iter().map(Ok::<_, Status>));
        Ok(Response::new(Box::pin(stream)))
    }

    async fn create_embedding(
        &self,
        _: Request<pb::CreateEmbeddingRequest>,
    ) -> Result<Response<pb::CreateEmbeddingResponse>, Status> {
        Err(unserved("create_embedding"))
    }

    async fn list_models(
        &self,
        _: Request<pb::ListModelsRequest>,
    ) -> Result<Response<pb::ListModelsResponse>, Status> {
        Err(unserved("list_models"))
    }

    async fn synthesize_speech(
        &self,
        _: Request<pb::SynthesizeSpeechRequest>,
    ) -> Result<Response<pb::SynthesizeSpeechResponse>, Status> {
        Err(unserved("synthesize_speech"))
    }

    async fn transcribe_speech(
        &self,
        _: Request<pb::TranscribeSpeechRequest>,
    ) -> Result<Response<pb::TranscribeSpeechResponse>, Status> {
        Err(unserved("transcribe_speech"))
    }

    async fn list_speech_voices(
        &self,
        _: Request<pb::ListSpeechVoicesRequest>,
    ) -> Result<Response<pb::ListSpeechVoicesResponse>, Status> {
        Err(unserved("list_speech_voices"))
    }

    async fn translate_text(
        &self,
        _: Request<pb::TranslateTextRequest>,
    ) -> Result<Response<pb::TranslateTextResponse>, Status> {
        Err(unserved("translate_text"))
    }

    async fn batch_translate_text(
        &self,
        _: Request<pb::BatchTranslateTextRequest>,
    ) -> Result<Response<pb::BatchTranslateTextResponse>, Status> {
        Err(unserved("batch_translate_text"))
    }

    async fn detect_text_language(
        &self,
        _: Request<pb::DetectTextLanguageRequest>,
    ) -> Result<Response<pb::DetectTextLanguageResponse>, Status> {
        Err(unserved("detect_text_language"))
    }

    async fn list_translation_languages(
        &self,
        _: Request<pb::ListTranslationLanguagesRequest>,
    ) -> Result<Response<pb::ListTranslationLanguagesResponse>, Status> {
        Err(unserved("list_translation_languages"))
    }

    async fn generate_image(
        &self,
        _: Request<pb::GenerateImageRequest>,
    ) -> Result<Response<pb::GenerateImageResponse>, Status> {
        Err(unserved("generate_image"))
    }

    async fn analyze_image(
        &self,
        _: Request<pb::AnalyzeImageRequest>,
    ) -> Result<Response<pb::AnalyzeImageResponse>, Status> {
        Err(unserved("analyze_image"))
    }

    async fn extract_image_text(
        &self,
        _: Request<pb::ExtractImageTextRequest>,
    ) -> Result<Response<pb::ExtractImageTextResponse>, Status> {
        Err(unserved("extract_image_text"))
    }

    async fn analyze_document(
        &self,
        _: Request<pb::AnalyzeDocumentRequest>,
    ) -> Result<Response<pb::AnalyzeDocumentResponse>, Status> {
        Err(unserved("analyze_document"))
    }

    async fn analyze_language(
        &self,
        _: Request<pb::AnalyzeLanguageRequest>,
    ) -> Result<Response<pb::AnalyzeLanguageResponse>, Status> {
        Err(unserved("analyze_language"))
    }

    async fn create_realtime_session(
        &self,
        _: Request<pb::CreateRealtimeSessionRequest>,
    ) -> Result<Response<pb::CreateRealtimeSessionResponse>, Status> {
        Err(unserved("create_realtime_session"))
    }

    async fn create_video_generation_job(
        &self,
        _: Request<pb::CreateVideoGenerationJobRequest>,
    ) -> Result<Response<pb::CreateVideoGenerationJobResponse>, Status> {
        Err(unserved("create_video_generation_job"))
    }

    async fn get_video_generation_job(
        &self,
        _: Request<pb::GetVideoGenerationJobRequest>,
    ) -> Result<Response<pb::GetVideoGenerationJobResponse>, Status> {
        Err(unserved("get_video_generation_job"))
    }

    async fn stream_video_generation_content(
        &self,
        _: Request<pb::StreamVideoGenerationContentRequest>,
    ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
        Err(unserved("stream_video_generation_content"))
    }

    async fn get_model_access(
        &self,
        _: Request<pb::GetModelAccessRequest>,
    ) -> Result<Response<pb::GetModelAccessResponse>, Status> {
        Err(unserved("get_model_access"))
    }
}

/// Serve `replay` on an ephemeral loopback port and return a channel to it. The server runs for
/// the rest of the test's runtime.
///
/// # Errors
///
/// Returns an error when the loopback listener cannot be bound or the channel cannot connect.
pub async fn serve(replay: ReplayInference) -> std::io::Result<Channel> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        Server::builder()
            .add_service(InferenceCoreServer::new(replay))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    Endpoint::from_shared(format!("http://{addr}"))
        .map_err(std::io::Error::other)?
        .connect()
        .await
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::*;

    const ONE_CALL: &str = r#"{"schema":"mp-replay-v1","id":"fixture","synthetic":true,"zdr":false,"description":"one call","tokenised_ids":[{"id":1,"prefix":"call_"}]}
{"call":1,"purpose":"ANSWER","expect":{"tools_offered":["lookup"],"last_role":"user"},"response":{"content":"ok {{id:1}} and {{id:1}}","stop_reason":"end_turn"}}"#;

    const STREAMED: &str = r#"{"schema":"mp-replay-v1","id":"streamed","synthetic":true,"zdr":false,"description":"one streamed answer","tokenised_ids":[]}
{"call":1,"purpose":"ANSWER","expect":{"tools_offered":[],"last_role":"user"},"response":{"content":"Hello","deltas":["Hel","lo"],"stop_reason":"end_turn"}}"#;

    fn request(tools: &[&str], last_role: &str) -> pb::InferRequest {
        pb::InferRequest {
            request_id: "req_fixture".to_owned(),
            model: "replay-model".to_owned(),
            purpose: pb::InferPurpose::Answer as i32,
            messages: vec![pb::ChatMessage {
                role: last_role.to_owned(),
                content: "hello".to_owned(),
                ..Default::default()
            }],
            tools: tools
                .iter()
                .map(|name| pb::ToolDefinition {
                    name: (*name).to_owned(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    fn fixture(source: &str) -> ReplayInference {
        ReplayInference::new(Cassette::parse(source).expect("fixture parses"))
    }

    #[test]
    fn cassette_loader_refuses_non_synthetic() {
        let unmarked = ONE_CALL.replace(r#""synthetic":true"#, r#""synthetic":false"#);
        assert_eq!(
            Cassette::parse(&unmarked),
            Err(ReplayError::NotSynthetic("fixture".to_owned()))
        );
        let absent = ONE_CALL.replace(r#""synthetic":true,"#, "");
        assert_eq!(
            Cassette::parse(&absent),
            Err(ReplayError::NotSynthetic("fixture".to_owned()))
        );
        let zdr = ONE_CALL.replace(r#""zdr":false"#, r#""zdr":true"#);
        assert_eq!(
            Cassette::parse(&zdr),
            Err(ReplayError::Zdr("fixture".to_owned()))
        );
    }

    #[test]
    fn cassette_refuses_an_undeclared_token() {
        let undeclared = ONE_CALL.replace(
            r#""tokenised_ids":[{"id":1,"prefix":"call_"}]"#,
            r#""tokenised_ids":[]"#,
        );
        assert!(matches!(
            Cassette::parse(&undeclared),
            Err(ReplayError::Invalid { .. })
        ));
    }

    #[test]
    fn cassette_refuses_an_unknown_purpose() {
        let unknown = ONE_CALL.replace(r#""purpose":"ANSWER""#, r#""purpose":"SUMMARISE""#);
        assert!(matches!(
            Cassette::parse(&unknown),
            Err(ReplayError::Invalid { .. })
        ));
    }

    #[tokio::test]
    async fn cassette_tokenised_ids_rebind() {
        let first = fixture(ONE_CALL);
        let second = fixture(ONE_CALL);
        let reply = first
            .infer(Request::new(request(&["lookup"], "user")))
            .await
            .expect("the request matches")
            .into_inner();
        let (a, b) = reply
            .content
            .strip_prefix("ok ")
            .and_then(|rest| rest.split_once(" and "))
            .expect("both occurrences are rendered");
        assert!(
            a.starts_with("call_"),
            "bound with the declared prefix: {a}"
        );
        assert_eq!(a, b, "one token keeps one value within a session");
        assert!(
            !reply.content.contains("{{id:"),
            "no token survives rebinding"
        );
        let other = second
            .infer(Request::new(request(&["lookup"], "user")))
            .await
            .expect("the request matches")
            .into_inner();
        assert_ne!(reply.content, other.content, "a new session binds new ids");
    }

    #[tokio::test]
    async fn replay_unmatched_request_fails_with_manifest_diff() {
        let replay = fixture(ONE_CALL);
        let status = replay
            .infer(Request::new(request(&["search"], "user")))
            .await
            .expect_err("the offered tools differ from the cassette");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert!(
            status.message().contains("\"lookup\""),
            "the expectation is named: {}",
            status.message()
        );
        assert!(
            status.message().contains("\"search\""),
            "the observed tool is named: {}",
            status.message()
        );
        let reason = replay
            .finish()
            .expect_err("the mismatch is kept for the test");
        assert!(reason.contains("does not match the request"), "{reason}");
        assert_eq!(replay.served(), 0);
    }

    #[tokio::test]
    async fn replay_refuses_a_call_with_another_purpose() {
        let replay = fixture(ONE_CALL);
        let mut title = request(&["lookup"], "user");
        title.purpose = pb::InferPurpose::Title as i32;
        let status = replay
            .infer(Request::new(title))
            .await
            .expect_err("a title call is not the recorded answer");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        let reason = replay
            .finish()
            .expect_err("the purpose mismatch is kept for the test");
        assert!(reason.contains("\"ANSWER\""), "{reason}");
    }

    #[tokio::test]
    async fn replay_refuses_a_call_after_the_cassette_ends() {
        let replay = fixture(ONE_CALL);
        replay
            .infer(Request::new(request(&["lookup"], "user")))
            .await
            .expect("the first call matches");
        assert_eq!(replay.finish(), Ok(()));
        let status = replay
            .infer(Request::new(request(&["lookup"], "user")))
            .await
            .expect_err("the cassette has no second call");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert!(replay.finish().is_err());
    }

    #[tokio::test]
    async fn replay_serves_the_recorded_stream() {
        let replay = fixture(STREAMED);
        let mut stream = replay
            .infer_stream(Request::new(request(&[], "user")))
            .await
            .expect("the request matches")
            .into_inner();
        let mut deltas = Vec::new();
        let mut last = None;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("a chunk");
            if chunk.done {
                last = Some(chunk);
            } else {
                deltas.push(chunk.delta);
            }
        }
        assert_eq!(deltas, ["Hel", "lo"]);
        assert_eq!(
            last.map(|chunk| chunk.stop_reason),
            Some("end_turn".to_owned())
        );
        assert_eq!(replay.finish(), Ok(()));
    }

    #[tokio::test]
    async fn replay_answers_over_the_grpc_boundary() {
        let replay = fixture(ONE_CALL);
        let channel = serve(replay.clone())
            .await
            .expect("the loopback server starts");
        let mut client = pb::inference_core_client::InferenceCoreClient::new(channel);
        let reply = client
            .infer(request(&["lookup"], "user"))
            .await
            .expect("the request matches over the wire")
            .into_inner();
        assert!(reply.content.starts_with("ok call_"), "{}", reply.content);
        assert_eq!(reply.request_id, "req_fixture");
        assert_eq!(reply.provider_used, PROVIDER);
        assert_eq!(replay.finish(), Ok(()));
    }
}
