//! The `generate_image` tool of the chat tool loop (Wave 4 W4-IMG-3, F-t3-051 step 1).
//!
//! A chat turn may ask for one image from a text description. The call goes to inference-core `GenerateImage` with
//! `strict_provider` set, so the one provider the request names answers it and no other provider is tried in its place.
//! The image reaches the person as an `artifact` of kind `image`, the event image mode emits. The model is told only the
//! artifact id, the model and provider that made it, the revised prompt and the size: never the bytes or the URL.
//!
//! # Who is offered it
//!
//! The chat composer offers the tool only on a persistent turn: not zero-retention, not waived, not conversation-only and
//! not a customer-gateway turn ([`composer_offers`]). The tool loop then admits it only on a turn that rides no
//! subscription route, since an image is metered API spend. It reads the org's capability policy for [`CAPABILITY_ID`]
//! once per turn, with the person's own capability bearer ([`policy_admits`]). A policy that withholds the tool (`never`,
//! or `ask`, which the Ask loop cannot answer for) withholds it, and so does a policy that cannot be read. The org's web
//! switch does not bind the tool: image generation is not public web access.
//!
//! # Limits and money
//!
//! At most [`MAX_IMAGES_PER_TURN`] provider calls per turn, one at a time. A call is counted as it is about to reach the
//! provider, so an invalid call costs nothing and a failed provider call still counts. A served call publishes an
//! `image_generation` usage event that cost-core prices per image (W4-IMG-2). A billed call is never retried.
//!
//! # Zero retention
//!
//! A zero-retention or waived turn is offered no tool, and a call that arrives anyway is refused with the typed
//! `zdr_feature_not_supported` refusal for `image_generation` before any provider call. The usage event carries no content.
//!
//! Off unless `MODEL_GATEWAY_IMAGE_TOOL` is the literal `true`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::Utc;
use mp_contracts::model_plane::v1::{
    GenerateImageRequest, GenerateImageResponse, ToolCall, ToolDefinition,
};
use mp_contracts::zdr_refusal::Refusal;
use mp_events::envelope::Envelope;
use mp_events::publisher::EventPublisher;
use mp_events::subjects;
use mp_ids::new_ulid;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use crate::artifacts::{artifact_event, ArtifactKind};
use crate::sse_events::ChatEvent;
use crate::state::AppState;
use crate::tool_loop::{err_outcome, with_authorization, ToolOutcome};

/// The tool's name, as offered and dispatched.
pub const TOOL_NAME: &str = "generate_image";

/// The switch. Only the literal `true` offers the tool.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_IMAGE_TOOL";

/// The capability whose tool policy governs the tool. capability-core names it "Image Generation and OCR".
pub const CAPABILITY_ID: &str = "cap.inference.image";

/// Images one turn may ask for. A call past the limit is refused, not queued.
pub const MAX_IMAGES_PER_TURN: u32 = 4;

/// The longest prompt the tool accepts, in characters.
const MAX_PROMPT_CHARS: usize = 4_000;

/// The longest model name the tool accepts, in characters.
const MAX_MODEL_CHARS: usize = 128;

/// The sizes the tool accepts.
const SIZES: [&str; 3] = ["1024x1024", "1024x1536", "1536x1024"];

/// The quality every call asks for. One image per call.
const QUALITY: &str = "standard";

/// The usage kind cost-core prices per image (W4-IMG-2).
const USAGE_KIND: &str = "image_generation";

/// The feature a zero-retention turn is refused for.
const FEATURE: &str = "image_generation";

/// The trailer value inference-core sets when a strict image call names no provider that serves it.
const IMAGE_MODEL_UNAVAILABLE: &str = "image_model_unavailable";

/// The longest a policy read may take on the turn's critical path. A slower answer withholds the tool.
const POLICY_TIMEOUT: Duration = Duration::from_secs(3);

/// The artifact title, in characters.
const TITLE_CHARS: usize = 200;

/// The revised prompt the model is told, in characters.
const MODEL_REVISED_PROMPT_CHARS: usize = 1_000;

/// The longest tool-call id copied into an artifact id, in characters.
const CALL_ID_CHARS: usize = 64;

/// The key of the output envelope a served call returns. The loop rewrites the envelope into the summary the model reads
/// (see [`image_events`]), so the bytes or the URL never reach the model.
const IMAGE_ENVELOPE_KEY: &str = "__verevon_image";

/// The arguments the model sends, read strictly: a field the schema does not name is refused.
const PARAMETERS_JSON: &str = r#"{"type":"object","properties":{"prompt":{"type":"string","minLength":1,"maxLength":4000,"description":"What the image shows, in plain words: the subject, the style and the composition."},"size":{"type":"string","enum":["1024x1024","1024x1536","1536x1024"],"description":"Pixels, width by height: 1024x1024 square, 1024x1536 portrait, 1536x1024 landscape."},"model":{"type":"string","maxLength":128,"description":"Optional image model name. Leave it out to use the default image model."}},"required":["prompt","size"],"additionalProperties":false}"#;

/// Whether `raw` (the switch's value, if set) is the literal on-word.
#[must_use]
pub fn parse_enabled(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether the switch is on in this process, read on every offer so a restart is not needed to turn it off.
#[must_use]
pub fn enabled() -> bool {
    parse_enabled(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// Whether `name` is the image tool.
#[must_use]
pub fn is_tool(name: &str) -> bool {
    name == TOOL_NAME
}

/// Whether a tool's calls are billed. A billed call is never retried, since a replay of one bills it twice.
#[must_use]
pub fn is_billed(name: &str) -> bool {
    is_tool(name)
}

/// Whether the chat composer offers the tool on a turn, before the org's policy is read: the switch is on, the turn is
/// persistent (neither zero-retention nor waived), it is not conversation-only and it is not a customer-gateway turn.
#[must_use]
pub fn composer_offers(
    enabled: bool,
    zdr: bool,
    retention_mode: bool,
    conversation_only: bool,
    customer_gateway: bool,
) -> bool {
    enabled && !zdr && !retention_mode && !conversation_only && !customer_gateway
}

/// Whether a turn's tool list carries the tool.
#[must_use]
pub fn offers(tools: &[ToolDefinition]) -> bool {
    tools.iter().any(|tool| is_tool(&tool.name))
}

/// The tool's place in a turn's tool list. With the switch off the list is left exactly as it was. With it on, a client's
/// copy of the name gives way to the server's own definition, which is kept only when `admitted`; otherwise the tool is
/// absent from the list.
#[must_use]
pub fn tools_for_turn(mut tools: Vec<ToolDefinition>, admitted: bool) -> Vec<ToolDefinition> {
    if !enabled() {
        return tools;
    }
    tools.retain(|tool| !is_tool(&tool.name));
    if admitted {
        tools.push(tool_definition());
    }
    tools
}

/// The tool as the model is offered it.
#[must_use]
pub fn tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: TOOL_NAME.to_owned(),
        description: format!(
            "Generate one image from a text description and show it to the person as an artifact. Use it only when they ask for an image, picture, drawing or illustration. The result gives the artifact_id, the model_used and provider_used that made it, the revised_prompt and the size. The person sees the image itself, so do not paste a link or describe the pixels; say briefly what you made. At most {MAX_IMAGES_PER_TURN} images per reply, one at a time."
        ),
        parameters_json: PARAMETERS_JSON.to_owned(),
    }
}

/// Whether the org's capability policy admits the tool for this person. capability-core's tool-policy list for
/// [`CAPABILITY_ID`] is read with the person's own capability bearer. A list that cannot be read withholds the tool, as does
/// any list that [`admits`] refuses.
pub async fn policy_admits(state: &AppState, capability_bearer: Option<&str>) -> bool {
    let Some(bearer) = capability_bearer.filter(|bearer| !bearer.trim().is_empty()) else {
        return false;
    };
    let url = format!(
        "{}/api/v1/tool-policies?capability_id={CAPABILITY_ID}",
        state.capability_core_base_url
    );
    let Ok(response) = state
        .http_client
        .get(url)
        .bearer_auth(bearer)
        .timeout(POLICY_TIMEOUT)
        .send()
        .await
    else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    match response.json::<Value>().await {
        Ok(body) => admits(&body),
        Err(_) => false,
    }
}

/// The tool-policy list's answer for [`CAPABILITY_ID`]. A list that does not enforce its policies admits the tool. An
/// enforced list admits it while every row on the capability allows it, so `never` and `ask` withhold it, and so does a
/// level this build does not know. A body of any other shape withholds it.
#[must_use]
pub fn admits(body: &Value) -> bool {
    let (Some(enforced), Some(policies)) = (
        body.get("enforced").and_then(Value::as_bool),
        body.get("policies").and_then(Value::as_array),
    ) else {
        return false;
    };
    if !enforced {
        return true;
    }
    policies
        .iter()
        .filter(|policy| policy.get("capability_id").and_then(Value::as_str) == Some(CAPABILITY_ID))
        .all(|policy| policy.get("decision").and_then(Value::as_str) == Some("allow"))
}

/// The images one turn has asked for: one provider call at a time, and a count of the calls begun.
struct TurnImages {
    /// One provider call in flight at a time, so a round that asks for several makes them in turn.
    slot: Semaphore,
    /// Provider calls begun this turn. Counted before the call, so a failed call still counts.
    begun: AtomicU32,
}

impl TurnImages {
    fn new() -> Self {
        Self {
            slot: Semaphore::new(1),
            begun: AtomicU32::new(0),
        }
    }

    /// Claims one of the turn's calls, or `false` once every call the turn may make has begun.
    fn begin(&self) -> bool {
        self.begun
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |begun| {
                (begun < MAX_IMAGES_PER_TURN).then_some(begun + 1)
            })
            .is_ok()
    }
}

/// Run id -> the turn's image budget, for as long as its [`ImageTurnGuard`] lives.
static TURNS: LazyLock<Mutex<HashMap<String, Arc<TurnImages>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn turns() -> MutexGuard<'static, HashMap<String, Arc<TurnImages>>> {
    TURNS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Holds a run's image budget while its tool rounds run. The budget is forgotten when the guard is dropped, so it never
/// outlives its turn.
#[must_use = "the image budget is forgotten as soon as the guard is dropped"]
pub struct ImageTurnGuard {
    /// The run and the budget this guard opened. Nothing is held when the tool was not admitted.
    held: Option<(String, Arc<TurnImages>)>,
}

impl Drop for ImageTurnGuard {
    fn drop(&mut self) {
        if let Some((run_id, budget)) = self.held.take() {
            // Forget only this guard's own budget: a later turn that opened the same run id keeps its entry.
            let mut map = turns();
            if map
                .get(&run_id)
                .is_some_and(|current| Arc::ptr_eq(current, &budget))
            {
                map.remove(&run_id);
            }
        }
    }
}

/// Opens the image budget of a run when the tool is `admitted` on it. A run the tool is not admitted on opens nothing, so
/// a call to the tool that arrives anyway is refused.
#[must_use]
pub fn hold_turn(run_id: &str, admitted: bool) -> ImageTurnGuard {
    if !admitted || run_id.trim().is_empty() {
        return ImageTurnGuard { held: None };
    }
    let budget = Arc::new(TurnImages::new());
    turns().insert(run_id.to_owned(), Arc::clone(&budget));
    ImageTurnGuard {
        held: Some((run_id.to_owned(), budget)),
    }
}

fn budget_of(run_id: &str) -> Option<Arc<TurnImages>> {
    turns().get(run_id).cloned()
}

/// One `generate_image` call's arguments, read strictly.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageArgs {
    prompt: String,
    size: String,
    #[serde(default)]
    model: Option<String>,
}

/// The arguments of one call, checked against the schema's limits.
#[derive(Debug, PartialEq, Eq)]
struct ImageRequest {
    prompt: String,
    size: &'static str,
    model: String,
}

impl ImageRequest {
    fn parse(arguments_json: &str) -> Result<Self, String> {
        let args: ImageArgs = serde_json::from_str(arguments_json).map_err(|_| {
            "generate_image needs a prompt and a size, may take a model, and no other fields"
                .to_owned()
        })?;
        let prompt = args.prompt.trim().to_owned();
        if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
            return Err(format!(
                "generate_image's prompt must be 1 to {MAX_PROMPT_CHARS} characters"
            ));
        }
        let size = SIZES
            .iter()
            .copied()
            .find(|size| *size == args.size.trim())
            .ok_or_else(|| {
                "generate_image's size must be 1024x1024, 1024x1536 or 1536x1024".to_owned()
            })?;
        let model = args.model.unwrap_or_default().trim().to_owned();
        if model.chars().count() > MAX_MODEL_CHARS || model.chars().any(char::is_control) {
            return Err(format!(
                "generate_image's model must be at most {MAX_MODEL_CHARS} characters, with no control characters"
            ));
        }
        Ok(Self {
            prompt,
            size,
            model,
        })
    }
}

/// What one `generate_image` call of a turn needs from the turn: the credential its provider call is sent with, and the
/// posture the turn was decided under.
pub(crate) struct ImageCall<'a> {
    pub state: &'a AppState,
    pub run_id: &'a str,
    pub org_id: &'a str,
    pub user_id: &'a str,
    pub inference_bearer: &'a str,
    pub zdr: bool,
    pub retention_mode: bool,
}

/// Serves one `generate_image` call. Every refusal is an error outcome the model can read. A served call returns the image
/// in an output envelope, which [`image_events`] turns into the artifact and the summary the model reads.
pub(crate) async fn dispatch(turn: &ImageCall<'_>, call: &ToolCall) -> ToolOutcome {
    if turn.zdr || turn.retention_mode {
        return refused(call, Refusal::feature_not_supported(FEATURE).json_body());
    }
    let Some(budget) = budget_of(turn.run_id) else {
        return err_outcome(call, "generate_image is not offered on this turn");
    };
    let request = match ImageRequest::parse(&call.arguments_json) {
        Ok(request) => request,
        Err(message) => return err_outcome(call, message),
    };
    if !budget.begin() {
        return refused(
            call,
            typed_error(
                "image_tool_limit_reached",
                &format!(
                    "At most {MAX_IMAGES_PER_TURN} images can be generated in one reply, and this reply has used them. Describe the images already made instead."
                ),
            ),
        );
    }
    let _slot = match budget.slot.acquire().await {
        Ok(permit) => permit,
        Err(_) => return err_outcome(call, "generate_image is not available on this turn"),
    };

    let provider_request = GenerateImageRequest {
        request_id: format!("{}-image-{}", turn.run_id, call.id),
        org_id: turn.org_id.to_owned(),
        prompt: request.prompt.clone(),
        model: request.model.clone(),
        provider_hint: String::new(),
        size: request.size.to_owned(),
        quality: QUALITY.to_owned(),
        n: 1,
        strict_provider: true,
    };
    let response = match generate(turn.state, provider_request, turn.inference_bearer).await {
        Ok(response) => response,
        Err(status) => return err_outcome(call, provider_failure(&status)),
    };
    let model_used = if response.model_used.is_empty() {
        request.model.clone()
    } else {
        response.model_used.clone()
    };
    publish_usage(
        turn,
        &call.id,
        &model_used,
        &response.provider_used,
        request.size,
    )
    .await;

    let Some(image) = response.images.into_iter().next() else {
        return err_outcome(
            call,
            provider_failure_message("The provider returned no image."),
        );
    };
    let content = if !image.url.is_empty() {
        image.url
    } else if !image.b64_json.is_empty() {
        format!("data:image/png;base64,{}", image.b64_json)
    } else {
        String::new()
    };
    if content.is_empty() {
        return err_outcome(
            call,
            provider_failure_message("The provider returned no image."),
        );
    }
    let revised_prompt = image.revised_prompt.trim().to_owned();
    let title = truncate(
        if revised_prompt.is_empty() {
            &request.prompt
        } else {
            &revised_prompt
        },
        TITLE_CHARS,
    );
    let mut envelope = serde_json::Map::new();
    envelope.insert(
        IMAGE_ENVELOPE_KEY.to_owned(),
        json!({
            "id": artifact_id(&call.id),
            "title": title,
            "content": content,
            "size": request.size,
            "model_used": model_used,
            "provider_used": response.provider_used,
            "revised_prompt": revised_prompt,
        }),
    );
    let output = Value::Object(envelope).to_string();
    ToolOutcome {
        call_id: call.id.clone(),
        name: call.name.clone(),
        provenance: crate::moderation::ToolProvenance::unscreened(&call.name, &output),
        output,
        error: None,
    }
}

/// The one place this tool calls inference-core: `GenerateImage` on the turn's own inference credential. A turn that reaches
/// it is persistent, since [`dispatch`] refuses a zero-retention or waived turn first.
async fn generate(
    state: &AppState,
    request: GenerateImageRequest,
    bearer: &str,
) -> Result<GenerateImageResponse, tonic::Status> {
    let mut client = state.inference_client.clone();
    client
        .generate_image(with_authorization(request, bearer))
        .await
        .map(tonic::Response::into_inner)
}

/// The typed error of a failed provider call: inference-core's own refusal when it named one, else one closed code. The
/// provider's text is never shown.
fn provider_failure(status: &tonic::Status) -> String {
    let refused_by_strict_choice = status
        .metadata()
        .get(crate::compaction::PROVIDER_ERROR_KIND_TRAILER)
        .and_then(|value| value.to_str().ok())
        == Some(IMAGE_MODEL_UNAVAILABLE);
    if refused_by_strict_choice {
        typed_error(
            "image_model_unavailable",
            "No configured image model serves this request, so no image was generated. No other provider was tried.",
        )
        .to_string()
    } else {
        provider_failure_message("The image could not be generated.")
    }
}

fn provider_failure_message(message: &str) -> String {
    typed_error("image_generation_failed", message).to_string()
}

/// The typed error body a refusal carries: `{"error": {"code", "message"}}`.
fn typed_error(code: &str, message: &str) -> Value {
    json!({ "error": { "code": code, "message": message } })
}

fn refused(call: &ToolCall, body: Value) -> ToolOutcome {
    err_outcome(call, body.to_string())
}

/// Publishes the usage of one served call for cost-core: an `image_generation` event naming the model, provider, size and
/// quality it was made at, which W4-IMG-2 prices per image. Best effort, as every usage envelope is. It carries no content.
async fn publish_usage(
    turn: &ImageCall<'_>,
    call_id: &str,
    model: &str,
    provider: &str,
    size: &str,
) {
    let envelope = Envelope {
        event_id: new_ulid(),
        event_type: "USAGE_ENVELOPE".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: turn.run_id.to_owned(),
        causation_id: String::new(),
        idempotency_key: format!("{}-USAGE_ENVELOPE-image-{}", turn.run_id, call_id),
        org_id: turn.org_id.to_owned(),
        user_id: turn.user_id.to_owned(),
        resource_ref: format!("request/{}", turn.run_id),
        payload: json!({
            "kind": USAGE_KIND,
            "request_id": turn.run_id,
            "org_id": turn.org_id,
            "user_id": turn.user_id,
            "run_id": turn.run_id,
            "model": model,
            "provider_used": provider,
            "input_tokens": 0,
            "output_tokens": 0,
            "cost_basis": "metered",
            "size": size,
            "quality": QUALITY,
            "n": 1,
            "transport": "tool_loop",
        }),
        zdr: false,
        ..Default::default()
    };
    if let Err(error) = turn
        .state
        .publisher
        .publish(&subjects::usage_subject(turn.org_id), &envelope)
        .await
    {
        tracing::warn!(%error, "failed to publish image_generation USAGE_ENVELOPE");
    }
}

/// The events a served call emits, and the summary the model reads in place of its envelope. A call that returned no
/// envelope emits nothing and keeps its output.
pub(crate) fn image_events(outcome: &ToolOutcome) -> (Vec<ChatEvent>, Option<String>) {
    let Some(image) = serde_json::from_str::<Value>(&outcome.output)
        .ok()
        .and_then(|value| value.get(IMAGE_ENVELOPE_KEY).cloned())
    else {
        return (Vec::new(), None);
    };
    let field = |key: &str| {
        image
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let id = field("id");
    let content = field("content");
    if id.is_empty() || content.is_empty() {
        return (Vec::new(), None);
    }
    let summary = json!({
        "artifact_id": id,
        "model_used": field("model_used"),
        "provider_used": field("provider_used"),
        "revised_prompt": truncate(&field("revised_prompt"), MODEL_REVISED_PROMPT_CHARS),
        "size": field("size"),
    });
    (
        vec![artifact_event(
            &id,
            ArtifactKind::Image,
            &field("title"),
            &content,
            1,
        )],
        Some(summary.to_string()),
    )
}

/// The artifact id of a call's image: the model's own call id, cleaned to the characters an id may hold.
fn artifact_id(call_id: &str) -> String {
    let cleaned: String = call_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(CALL_ID_CHARS)
        .collect();
    if cleaned.is_empty() {
        "image".to_owned()
    } else {
        format!("{cleaned}-image")
    }
}

/// The first `max` characters of `text`, never splitting one.
fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
