//! Opt-in fault-injection controls for context-sensitive source rechecks.
//! The known prior assessment and private repair are scripted; ONLY the
//! subsequent recheck uses live inference. The separately labelled inline-repair
//! control uses live reviews throughout; none is an authoring benchmark.
use base64::Engine;
use model_gateway::source_validation::{check_artifact, Source, SourceContext};
use mp_contracts::model_plane::v1::{inference_core_client::InferenceCoreClient, InferRequest, InferResponse};
use serde_json::{json, Value};
use std::sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Mutex};
use std::time::{Duration, Instant};

fn rejects_dependent_passage(report: &Value) -> bool {
    report["checks"].as_array().is_some_and(|checks| checks.iter().any(|check| {
        check.get("index").or_else(|| check.get("i")).and_then(Value::as_u64) == Some(2)
            && matches!(check.get("status").or_else(|| check.get("v")).and_then(Value::as_str), Some("unsupported" | "u"))
            && check.get("reason").or_else(|| check.get("r")).and_then(Value::as_str).is_some_and(|reason| !reason.trim().is_empty())
    }))
}

#[test]
fn dependent_rejection_diagnostic_accepts_both_wire_formats() {
    for check in [
        json!({"index":2,"status":"unsupported","reason":"Changed referent conflicts with source."}),
        json!({"i":2,"v":"u","r":"Changed referent conflicts with source."}),
    ] {
        assert!(rejects_dependent_passage(&json!({"checks":[check]})));
    }
    for check in [
        json!({"i":1,"v":"u","r":"Only the edited paragraph is wrong."}),
        json!({"i":2,"v":"s","r":"Supported."}),
        json!({"i":2,"v":"u","r":" "}),
        json!({"index":2,"status":"unsupported"}),
    ] {
        assert!(!rejects_dependent_passage(&json!({"checks":[check]})));
    }
}

#[tokio::test]
#[ignore = "uses the local ChatGPT Terra subscription; explicit evidence paths required"]
async fn context_changes_must_invalidate_otherwise_identical_passages() -> anyhow::Result<()> {
    let state: Value = serde_json::from_slice(&std::fs::read(std::env::var("VEREVON_REVIEW_EVAL_STATE")?)?)?;
    let report_path = std::env::var("VEREVON_RECHECK_EVAL_REPORT")?;
    let cookies = state["cookies"].as_array().ok_or_else(|| anyhow::anyhow!("local session required"))?.iter()
        .filter(|c| matches!(c["domain"].as_str(), Some("localhost" | ".localhost")))
        .filter_map(|c| Some(format!("{}={}", c["name"].as_str()?, c["value"].as_str()?)))
        .collect::<Vec<_>>().join("; ");
    anyhow::ensure!(!cookies.is_empty(), "local authenticated session required");
    let http = reqwest::Client::new();
    let client = InferenceCoreClient::connect("http://127.0.0.1:9092").await?;
    let cases = [
        ("reference-preserved", "The Ridge package includes screws. The Lake package includes brackets and excludes screws.",
            "The Ridge package includes screws and weighs 10 kg.", "It includes screws.", "The Ridge package includes screws.", true, false),
        ("reference-changed", "The Ridge package includes screws. The Lake package includes brackets and excludes screws.",
            "The Ridge package includes screws and weighs 10 kg.", "It includes screws.", "The Lake package includes brackets.", false, false),
        ("sample-preserved", "We interviewed six school leaders. Four of these six requested desks. Preferences outside this sample are unknown.",
            "We interviewed six school leaders who all have green offices.", "Most of them requested desks.", "We interviewed six school leaders.", true, false),
        ("sample-expanded", "We interviewed six school leaders. Four of these six requested desks. Preferences outside this sample are unknown.",
            "We interviewed six school leaders who all have green offices.", "Most of them requested desks.", "Schools employ school leaders.", false, false),
        ("independent-section-preserved", "The Ridge package includes screws. The Lake package includes brackets and excludes screws.",
            "The Ridge package includes screws and weighs 10 kg.", "It includes screws.", "The Ridge package includes screws.", true, true),
        ("dependent-claim-with-independent-section", "The Ridge package includes screws. The Lake package includes brackets and excludes screws.",
            "The Ridge package includes screws and weighs 10 kg.", "It includes screws.", "The Lake package includes brackets.", false, true),
        ("schedule-reference-preserved", "The Atlas exhibition opens on 12 October. The Meadow exhibition opens on 20 October.",
            "The Atlas exhibition opens on 12 October and has green offices.", "It opens on 12 October.", "The Atlas exhibition opens on 12 October.", true, false),
        ("schedule-reference-changed", "The Atlas exhibition opens on 12 October. The Meadow exhibition opens on 20 October.",
            "The Atlas exhibition opens on 12 October and has green offices.", "It opens on 12 October.", "The Meadow exhibition opens on 20 October.", false, false),
        ("inline-repair-live", "The parcel weighs 4 kg.",
            "The parcel weighs 10 kg.", "Please review before use.", "The parcel weighs 4 kg.", true, false),
        ("inline-reference-live", "The Atlas exhibition opens on 12 October. The Meadow exhibition opens on 20 October.",
            "The Meadow exhibition opens on 20 October.", "It opens on 12 October.", "The Meadow exhibition opens on 20 October.", true, false),
    ];
    let mut results = Vec::new();
    for (id, source, faulty, dependent, replacement, expected, independent) in cases {
        let live_initial = matches!(id, "inline-repair-live" | "inline-reference-live");
        let response = http.get("http://localhost:3011/api/inference-core/token").header("Cookie", &cookies).send().await?;
        anyhow::ensure!(response.status().is_success(), "inference authentication unavailable");
        let response: Value = response.json().await?;
        let token = response["token"].as_str().ok_or_else(|| anyhow::anyhow!("token missing"))?;
        let claims: Value = serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(
            token.split('.').nth(1).ok_or_else(|| anyhow::anyhow!("token format"))?)?)?;
        anyhow::ensure!(claims["zdr"].as_bool() != Some(true), "subscription is unavailable under this account policy");
        let org = claims["org_id"].as_str().ok_or_else(|| anyhow::anyhow!("active org missing"))?;
        let response = http.get("http://localhost:5173/api/v1/integrations/connections")
            .header("Cookie", &cookies).header("x-verevon-org-id", org).send().await?;
        anyhow::ensure!(response.status().is_success(), "subscription connections unavailable");
        let connections: Value = response.json().await?;
        let data = connections.get("data").unwrap_or(&connections);
        let rows = data.as_array().or_else(|| data["connections"].as_array()).ok_or_else(|| anyhow::anyhow!("connection response invalid"))?;
        let connection = rows.iter().find(|row|
            row.get("providerKey").or_else(|| row.get("provider_key")).or_else(|| row.get("providerId")).and_then(Value::as_str) == Some("openai-codex-subscription")
                && row["status"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("active"))
                && row["deletedAt"].as_str().is_none_or(str::is_empty) && row["deleted_at"].as_str().is_none_or(str::is_empty))
            .and_then(|row| row["id"].as_str()).ok_or_else(|| anyhow::anyhow!("active ChatGPT subscription required; no fallback"))?;
        let request = InferRequest { request_id:format!("recheck-eval-{}-{id}", chrono::Utc::now().timestamp_millis()),
            org_id:org.into(), model:"gpt-5.6-terra".into(), provider_hint:"openai-codex-subscription".into(),
            subscription_connection_id:connection.into(), ..Default::default() };
        let authorization: tonic::metadata::MetadataValue<_> = format!("Bearer {token}").parse()?;
        let context = SourceContext { sources:vec![Source { id:0,name:"independent-context.txt".into(),content:source.into() }] };
        let candidate = format!("# Note\n\n{faulty}\n\n{dependent}{}", if independent { "\n\n# Review note\n\nPlease review before use." } else { "" });
        let calls = Arc::new(AtomicUsize::new(0));
        let repairs = Arc::new(AtomicUsize::new(0));
        let violated = Arc::new(AtomicBool::new(false));
        let responses = Arc::new(Mutex::new(Vec::<Value>::new()));
        let injected = Arc::new(Mutex::new(Vec::<Value>::new()));
        let mut infer = |request: InferRequest| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            let data: Value = serde_json::from_str(&request.messages.iter().rev().find(|m| m.role == "user").unwrap().content).unwrap();
            let mut client = client.clone();
            let authorization = authorization.clone();
            let responses = responses.clone();
            let injected = injected.clone();
            let violated = violated.clone();
            let repairs = repairs.clone();
            async move {
                if request.model != "gpt-5.6-terra" || request.provider_hint != "openai-codex-subscription" || request.subscription_connection_id.is_empty() {
                    violated.store(true, Ordering::SeqCst);
                    return Err(tonic::Status::failed_precondition("blocked non-subscription recheck request"));
                }
                // Negative fault controls measure the first recheck's ability
                // to invalidate unchanged text. Stop afterward so a newly
                // suggested repair cannot turn this deliberate bad edit into
                // a different, corrected document and hide the observation.
                if !expected && call > 2 {
                    injected.lock().unwrap().push(json!({"stage":"stop_after_fault_recheck"}));
                    return Err(tonic::Status::failed_precondition(model_gateway::result_validation::VALIDATION_FAILED));
                }
                let seeded = if live_initial { None } else if call == 0 {
                    let mut checks = vec![
                        json!({"index":0,"status":"no_assertions","evidence":[],"reason":""}),
                        json!({"index":1,"status":"unsupported","evidence":[],"reason":"The added weight or office detail is undocumented."}),
                        json!({"index":2,"status":"supported","evidence":[{"source":0,"lines":[0]}],"reason":""}),
                    ];
                    if independent {
                        checks.extend([json!({"index":3,"status":"no_assertions","evidence":[],"reason":""}), json!({"index":4,"status":"no_assertions","evidence":[],"reason":""})]);
                    }
                    Some(("prior_review", json!({"checks":checks})))
                } else if request.request_id.contains("-artifact-repair-") {
                    let round = repairs.fetch_add(1, Ordering::SeqCst);
                    let patches = if round == 0 { vec![json!({"index":1,"text":replacement})] }
                    else { data["requiredRepairs"].as_array().unwrap().iter().map(|failure| {
                        let index = failure["index"].as_u64().unwrap() as usize;
                        json!({"index":index,"text":data["segments"][index]["text"]})
                    }).collect() };
                    Some(("private_repair", json!({"repairs":patches})))
                } else { None };
                if let Some((stage, report)) = seeded {
                    injected.lock().unwrap().push(json!({"stage":stage,"report":report}));
                    return Ok(InferResponse { content:report.to_string(),stop_reason:"end_turn".into(),..Default::default() });
                }
                let effort = request.thinking_budget_tokens;
                let previous_count = data["previousChecks"].as_array().map_or(0, Vec::len);
                let repair_call = request.request_id.contains("-artifact-repair-");
                let output_schema = serde_json::from_str::<Value>(&request.structured_output_schema).ok();
                let mut request = tonic::Request::new(request);
                request.metadata_mut().insert("authorization", authorization);
                let response = model_gateway::result_validation::infer_candidate(&mut client, request).await?;
                if response.model_used != "gpt-5.6-terra" || response.provider_used != "openai-codex-subscription" {
                    violated.store(true, Ordering::SeqCst);
                    return Err(tonic::Status::failed_precondition("recheck route changed"));
                }
                responses.lock().unwrap().push(json!({"content":response.content,"model":response.model_used,
                    "provider":response.provider_used,"requestedThinkingBudgetTokens":effort,"previousChecks":previous_count,"stopReason":response.stop_reason,
                    "repairCall":repair_call,"requestedOutputSchema":output_schema}));
                Ok(response)
            }
        };
        let started = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(90), check_artifact(Some(&context), &request,
            "Write a note using only the supplied attachment.", None, &candidate, &mut infer)).await;
        let (accepted, content, receipt, error) = match result {
            Ok(Ok((content, receipt))) => (Some(true), Some(content), receipt, None),
            Ok(Err(status)) if status.message() == model_gateway::result_validation::VALIDATION_FAILED =>
                (Some(false), None, None, Some("validation_failed".to_owned())),
            Ok(Err(status)) => (None, None, None, Some(format!("inference_or_protocol:{:?}", status.code()))),
            Err(_) => (None, None, None, Some("timeout".to_owned())),
        };
        let live = responses.lock().unwrap();
        let invalidated_unchanged_passage = live.iter().any(|response| {
            let Ok(report) = serde_json::from_str::<Value>(response["content"].as_str().unwrap_or_default()) else { return false; };
            rejects_dependent_passage(&report)
        });
        // A timeout, malformed report or rejection of only the edited paragraph
        // is NOT evidence that the unchanged dependent claim was re-evaluated.
        let expected_content = if id == "inline-reference-live" { candidate.replace("It opens on 12 October.", "It opens on 20 October.") }
            else { candidate.replacen(faulty, replacement, 1) };
        let private_proposal = live.first().and_then(|response| serde_json::from_str::<Value>(response["content"].as_str()?).ok())
            .is_some_and(|report| report["checks"].as_array().is_some_and(|checks| checks.iter().any(|check| check["x"].as_array().is_some_and(|edits| !edits.is_empty()))));
        let correct = accepted == Some(expected) && !live.is_empty()
            && live.iter().all(|response| response["previousChecks"] == if independent { 2 } else { 0 })
            && (expected || invalidated_unchanged_passage)
            && (!expected || content.as_deref() == Some(expected_content.as_str()))
            && (id != "inline-reference-live" || invalidated_unchanged_passage)
            && (!live_initial || (live.len() >= 2 && private_proposal && live.iter().all(|response| response["repairCall"] == false)));
        results.push(json!({"id":id,"expectedAccepted":expected,"accepted":accepted,"correct":correct,"elapsedMs":started.elapsed().as_millis(),
            "content":content,"receipt":receipt,"error":error,"invalidatedUnchangedPassage":invalidated_unchanged_passage,
            "liveInitialReview":live_initial,"privateProposal":private_proposal,
            "injected":*injected.lock().unwrap(),"responses":*live}));
        drop(live);
        std::fs::write(&report_path, serde_json::to_vec_pretty(&results)?)?;
        println!("{id}: correct={correct} accepted={accepted:?}");
        anyhow::ensure!(!violated.load(Ordering::SeqCst), "route violation; stopped evaluation");
    }
    anyhow::ensure!(results.iter().all(|row| row["correct"] == true), "context-sensitive recheck controls failed");
    Ok(())
}
