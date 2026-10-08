//! The live proof of the contextualizer. NOT run by `cargo test`: every test here is
//! `#[ignore]` and makes real calls. It is run by the orchestrator, once, through
//! `tests/scripts/contextual-retrieval-live-proof.sh`, against a throwaway
//! organization (see the runbook, section "Live proof of contextualizer v2").
//!
//! What it does, in order, and what each step costs:
//!
//! 1. **Preflight (free).** Mints a bearer for the organization through the real
//!    token client, reads its `zdr` claim, and asks inference-core's `ListModels`
//!    (a catalogue read, no model call) whether the Azure deployment that serves the
//!    configured model declares a residency that meets the configured floor. A floor
//!    nobody can meet is found here, for free, instead of as a refusal on every call.
//! 2. **Three calls (paid, about 0.002 USD).** One synthetic Norwegian document of
//!    about 1,200 tokens, three chunks. The first chunk goes alone and the other two
//!    follow, so the provider's prefix cache is warm for them. Every raw response is
//!    recorded.
//! 3. **One call (paid, a fraction of a cent).** The same chunk with a 16-token
//!    output cap, which makes the provider answer `finish_reason: length`: the real
//!    truncated case.
//!
//! Then it checks, and prints, the pass criteria below, and (with
//! `CR_LIVE_WRITE_CAPTURE=1`) writes the real answers to
//! `tests/fixtures/contracts/infer-response-azure-live.json` with its pin, which is
//! what the validation tests are held against from then on.
//!
//! The document is synthetic (an invented company), so printing its contexts is
//! printing nothing of a customer's. Nothing else is printed that could be a secret:
//! no token, no key, no URL with a credential.
//!
//! Pass criteria
//!
//! * the bearer's `zdr` claim is false for the throwaway organization;
//! * the model is served by an Azure OpenAI deployment that declares a residency
//!   meeting the floor;
//! * all three outcomes are `Ready`, `provider_used` names Azure OpenAI,
//!   `residency` is the floor's, `model_used` starts with the configured model;
//! * every accepted answer's `stop_reason` is `stop` (or `end_turn`), and the
//!   length probe's is `length`;
//! * no context opens with "This chunk", "The chunk" or "This section" (a count,
//!   reported; a failure only under ctx-v2, whose prompt forbids it);
//! * `cache_read_input_tokens` is MEASURED and printed. It is not a pass
//!   criterion: Azure's automatic prefix cache needs a 1,024-token prefix and is
//!   best effort, so zero is a result, not a failure;
//! * one request reached Auth Core for the four calls (the bearer is cached), and
//!   exactly four provider calls were made.

use std::sync::{Arc, Mutex, PoisonError};

use serde_json::json;

use super::*;
use crate::context::validate::{classify_stop_reason, StopKind};

/// Where the proof writes the real answers.
const CAPTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/contracts/infer-response-azure-live.json"
);

/// Records every raw response of the port it wraps.
struct Recording {
    inner: GrpcInference,
    responses: Mutex<Vec<model_plane::v1::InferResponse>>,
}

impl Recording {
    fn recorded(&self) -> Vec<model_plane::v1::InferResponse> {
        self.responses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl InferencePort for Recording {
    fn infer(&self, request: tonic::Request<model_plane::v1::InferRequest>) -> InferFuture<'_> {
        let call = self.inner.infer(request);
        Box::pin(async move {
            let response = call.await?;
            self.responses
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(response.clone());
            Ok(response)
        })
    }
}

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("the live proof needs {name} in the environment"))
}

/// An invented quarterly report: about 1,200 tokens of Norwegian, with three
/// paragraphs that each need the document to be understood.
fn synthetic_document() -> (String, [String; 3]) {
    let paragraphs = [
        "Fjordfisk AS er et familieeid selskap i Tromsø som drev med oppdrett, røyking og salg av laks i 2025. Selskapet hadde i alt 146 ansatte ved årsskiftet, fordelt på tre anlegg i Troms og ett pakkeri i Narvik.",
        "Styret legger med dette frem årsrapporten for regnskapsåret 2025. Rapporten omfatter konsernregnskapet, noter, revisors beretning og styrets egen vurdering av risiko og fremtidsutsikter.",
        "Driftsmarginen økte fra 24 til 31 prosent. Økningen skyldes lavere fôrkostnader, bedre utnyttelse av røykeriet og en gunstig utvikling i lakseprisen gjennom de to siste kvartalene.",
        "Salgsinntektene utgjorde 412 millioner kroner, mot 371 millioner året før. Eksport til Sverige og Danmark sto for 38 prosent av salget, mens hjemmemarkedet sto for resten.",
        "Biologien var god gjennom hele året. Dødeligheten i merdene var 2,1 prosent, som er den laveste siden selskapet startet opp, og lusenivået holdt seg under myndighetenes grense i alle ukene.",
        "Fôrkostnadene falt med seks prosent per produsert kilo etter at selskapet byttet leverandør i mars. Leveringsavtalen med den nye leverandøren løper til utgangen av 2027.",
        "Røykeriet i Tromsø ble utvidet med to nye anlegg i løpet av høsten, og kapasiteten økte med 40 prosent. Investeringen på 18 millioner kroner ble finansiert av egne midler og et lån fra Innovasjon Norge.",
        "Pakkeriet i Narvik fikk ny linje for vakuumpakking i juni. Linjen reduserte svinnet med en tredjedel og gjorde det mulig å levere ferdigpakkede porsjoner til dagligvarekjeder.",
        "Sykefraværet var 3,4 prosent, som er under bransjegjennomsnittet. Selskapet satte i gang et nytt program for opplæring av lærlinger, og tolv lærlinger fikk fast ansettelse etter endt læretid.",
        "Styret er opptatt av bærekraft og har satt som mål å halvere utslippene per kilo laks innen 2030. I 2025 ble det installert landstrøm på to av tre anlegg, noe som kuttet dieselforbruket tydelig.",
        "Selskapet har ingen virksomhet innen forskning og utvikling utover løpende forbedringer av produksjonen. Styret anser ikke at selskapet er utsatt for vesentlig valutarisiko ut over salg i svenske og danske kroner.",
        "Fremtidsutsiktene er gode. Styret forventer en driftsmargin mellom 28 og 32 prosent i 2026, og at det nye røykeriet vil gi økt omsetning fra første kvartal.",
    ];
    let document = format!(
        "Årsrapport 2025 for Fjordfisk AS\n\n{}",
        paragraphs.join("\n\n")
    );
    let chunks = [
        paragraphs[2].to_string(),
        paragraphs[6].to_string(),
        paragraphs[11].to_string(),
    ];
    (document, chunks)
}

fn engine_over(
    cfg: &Config,
    token_client: &InferenceTokenClient,
    tweak: impl FnOnce(&mut Settings),
) -> (Contextualizer, Arc<Recording>) {
    let mut settings = Settings::from_config(cfg).expect("a valid configuration");
    tweak(&mut settings);
    let recording = Arc::new(Recording {
        inner: grpc_inference(cfg, &settings).expect("a valid inference-core URL"),
        responses: Mutex::new(Vec::new()),
    });
    (
        Contextualizer::new(recording.clone(), token_client.clone(), settings),
        recording,
    )
}

fn residency_rank(label: &str) -> u8 {
    match label {
        "norway" | "no" | "sovereign" => 2,
        "eu" | "eea" => 1,
        _ => 0,
    }
}

fn case_of(name: &str, response: &model_plane::v1::InferResponse) -> serde_json::Value {
    let usage = usage_of(response);
    json!({
        "name": name,
        "stop_reason": response.stop_reason,
        "model_used": response.model_used,
        "provider_used": response.provider_used,
        "residency": response.residency,
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "cache_read_input_tokens": usage.cache_read_input_tokens,
        "usage_reported": usage.usage_reported,
        "response_cache_hit": usage.response_cache_hit,
        "content": response.content,
    })
}

#[tokio::test]
#[ignore = "live: makes four paid calls; run through tests/scripts/contextual-retrieval-live-proof.sh"]
async fn contextualizer_v2_live_proof() {
    let org_id = required("CR_LIVE_ORG_ID");
    let mut cfg = Config::from_env().expect("the embedding-engine environment");
    cfg.contextual_retrieval_enabled = true;
    let (document, chunks) = synthetic_document();

    // 1. Preflight, free. One token client for the whole proof, as in the service: the
    // bearer is minted once and served from its cache to every call.
    let token_client = super::super::token_client_from_config(&cfg).expect("token client");
    let (engine, recording) = engine_over(&cfg, &token_client, |_| {});
    let bearer = token_client
        .bearer(&org_id, Reason::ContextualRetrieval)
        .await
        .expect("Auth Core issues a bearer for the throwaway organization");
    println!("preflight: bearer minted, zdr claim = {}", bearer.zdr());
    assert!(
        !bearer.zdr(),
        "the throwaway organization must not be zero-retention"
    );
    let floor = engine.settings.min_residency.clone();
    let model = engine.settings.model.clone();
    let channel = tonic::transport::Endpoint::from_shared(cfg.model_plane_ai_core_grpc_url.clone())
        .expect("URL")
        .connect()
        .await
        .expect("inference-core is reachable");
    let mut catalogue = model_plane::v1::inference_core_client::InferenceCoreClient::new(channel);
    let mut list = tonic::Request::new(model_plane::v1::ListModelsRequest {
        modality: "chat".to_string(),
        provider: engine.settings.provider_hint.clone(),
    });
    list.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", bearer.as_str())
            .parse()
            .expect("header"),
    );
    let listed = catalogue
        .list_models(list)
        .await
        .expect("inference-core lists its models")
        .into_inner();
    let served: Vec<_> = listed
        .models
        .iter()
        .filter(|info| info.id == model)
        .collect();
    for info in &served {
        println!(
            "preflight: model {} is served by provider {} declaring residency {:?}",
            info.id, info.provider, info.residency
        );
    }
    assert!(
        !served.is_empty(),
        "inference-core lists no chat model `{model}` for provider `{}`",
        engine.settings.provider_hint
    );
    if floor != "global" {
        assert!(
            served
                .iter()
                .any(|info| residency_rank(&info.residency) >= residency_rank(&floor)),
            "no deployment of `{model}` declares a residency of at least `{floor}`: \
             every call would be refused (is AZURE_OPENAI_REGION set on inference-core?)"
        );
    }

    if std::env::var_os("CR_LIVE_PREFLIGHT_ONLY").is_some() {
        println!("PASS: preflight only (no paid call was made)");
        return;
    }

    // 2. The three paid calls.
    let prepared = engine.prepare(DocumentInput {
        org_id: &org_id,
        document_id: "live-proof-synthetic",
        meta: DocumentMeta {
            title: "Årsrapport 2025 for Fjordfisk AS",
            source: "",
            date: "",
        },
        text: &document,
        restricted: false,
        prompt_version: None,
        total_chunks: Some(12),
    });
    let inputs: Vec<ChunkInput<'_>> = chunks
        .iter()
        .enumerate()
        .map(|(index, text)| ChunkInput {
            index: i32::try_from(index * 5 + 2).expect("small"),
            text,
        })
        .collect();
    let results = engine.contextualize(&prepared, &inputs, None).await;
    let raw = recording.recorded();
    println!(
        "calls 1-3: {} provider calls, prompt {}, {} document characters, cacheable window = {}",
        raw.len(),
        prepared.version().as_str(),
        document.chars().count(),
        document.len() >= PREFIX_CACHE_MIN_BYTES
    );
    assert_eq!(raw.len(), 3, "exactly three provider calls");
    let mut meta_openings = 0;
    for (index, result) in results.iter().enumerate() {
        let generated = result
            .outcome
            .generated()
            .unwrap_or_else(|| panic!("chunk {index} is {:?}", result.outcome));
        println!(
            "chunk {index}: status={} model_used={} provider_used={} residency={} input_tokens={} output_tokens={} \
             cache_read_input_tokens={} response_cache_hit={} meta_opening={}",
            result.outcome.status().as_str(),
            generated.model_used,
            generated.provider_used,
            generated.residency,
            generated.usage.input_tokens,
            generated.usage.output_tokens,
            generated.usage.cache_read_input_tokens,
            generated.usage.response_cache_hit,
            generated.meta_opening,
        );
        println!(
            "chunk {index} context (synthetic document): {}",
            generated.text
        );
        meta_openings += usize::from(generated.meta_opening);
        assert!(
            generated
                .provider_used
                .to_ascii_lowercase()
                .contains("azure"),
            "provider_used is `{}`",
            generated.provider_used
        );
        assert!(
            generated.model_used.starts_with(&model),
            "model_used is `{}`",
            generated.model_used
        );
        if floor != "global" {
            assert!(
                residency_rank(&generated.residency) >= residency_rank(&floor),
                "residency `{}` does not meet the floor `{floor}`",
                generated.residency
            );
        }
    }
    for response in &raw {
        assert_eq!(
            classify_stop_reason(&response.stop_reason),
            StopKind::Complete,
            "the real finish reason is `{}`",
            response.stop_reason
        );
    }
    println!(
        "MEASURED (not a gate): cache_read_input_tokens over calls 2 and 3 = {} (calls: {:?})",
        raw.iter()
            .skip(1)
            .map(|r| usage_of(r).cache_read_input_tokens)
            .sum::<u32>(),
        raw.iter()
            .map(|r| usage_of(r).cache_read_input_tokens)
            .collect::<Vec<_>>()
    );
    if prepared.version() == PromptVersion::V2 {
        assert_eq!(meta_openings, 0, "ctx-v2 forbids meta openings");
    }
    println!("meta openings: {meta_openings} of 3");

    // 3. The real truncated case.
    let (short_engine, short_recording) = engine_over(&cfg, &token_client, |settings| {
        settings.max_tokens = 16;
        settings.retry_attempts = 0;
    });
    let short = short_engine
        .contextualize(&prepared, &inputs[..1], None)
        .await;
    let short_raw = short_recording.recorded();
    assert_eq!(short_raw.len(), 1, "one provider call for the length probe");
    println!(
        "length probe: finish_reason={:?} outcome={:?}",
        short_raw[0].stop_reason,
        short[0].outcome.status()
    );
    assert_eq!(
        classify_stop_reason(&short_raw[0].stop_reason),
        StopKind::Truncated,
        "a 16-token cap must make the provider answer `length`; got `{}`",
        short_raw[0].stop_reason
    );

    // One request to Auth Core served all four provider calls: the bearer is cached.
    let stats = token_client.stats();
    println!(
        "auth core: {} mint(s), {} served from the cache, for {} provider calls",
        stats.mints,
        stats.hits,
        raw.len() + short_raw.len()
    );
    assert_eq!(stats.mints, 1, "the bearer is minted once and cached");

    if std::env::var_os("CR_LIVE_WRITE_CAPTURE").is_some() {
        let cases = [
            case_of("chunk_1", &raw[0]),
            case_of("chunk_2", &raw[1]),
            case_of("chunk_3", &raw[2]),
            case_of("length_probe", &short_raw[0]),
        ];
        let capture = json!({
            "contract": "inference-core.infer-response.azure-openai",
            "version": 1,
            "captured_at": chrono::Utc::now().to_rfc3339(),
            "captured_with": "embedding-engine-rs provider::contextualize::live_proof (synthetic document, throwaway organization)",
            "model": model,
            "min_residency": floor,
            "cases": cases,
        });
        let mut text = serde_json::to_string_pretty(&capture).expect("encode");
        text.push('\n');
        std::fs::write(CAPTURE_PATH, text.as_bytes()).expect("write the capture");
        let digest = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(text.as_bytes()))
        };
        std::fs::write(
            format!("{CAPTURE_PATH}.sha256"),
            format!("{digest}  infer-response-azure-live.json\n"),
        )
        .expect("write the pin");
        println!("capture written: infer-response-azure-live.json (sha256 {digest})");
    }
    println!("PASS: contextualizer v2 live proof (4 paid calls)");
}
