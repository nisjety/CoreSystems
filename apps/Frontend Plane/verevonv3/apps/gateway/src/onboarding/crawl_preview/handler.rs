use async_stream::stream;
use axum::{
    extract::State,
    http::HeaderMap,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Extension, Json,
};
use serde_json::{json, Value};
use tracing::error;

use crate::{
    audience_tokens::get_onboarding_preview_token,
    config::AppState,
    contracts::CrawlPreviewRequest,
    middleware::AuthenticatedUser,
    onboarding::crawl_preview::{
        dedupe::{Admission, SnippetLedger},
        quarry::{create_crawl_job, forward_seed_scrape, poll_crawl_events},
        sse::sse_json,
    },
    public_url::normalize_public_http_url,
};

pub(crate) async fn crawl_preview(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<CrawlPreviewRequest>,
) -> impl IntoResponse {
    let url = match normalize_public_http_url(&input.url) {
        Ok(url) => url,
        Err(message) => {
            let output = stream! {
                yield Ok::<Event, std::convert::Infallible>(sse_json("warning", json!({ "code": "invalid_url", "message": message })));
                yield Ok(sse_json("done", json!({ "count": 0, "pages": 0, "elements": 0, "status": "failed" })));
            };
            return Sse::new(output)
                .keep_alive(KeepAlive::default())
                .into_response();
        }
    };

    let max_pages = input.max_pages.unwrap_or(6).clamp(1, 12);
    // Mint the pre-org onboarding preview token up front (before the SSE stream
    // starts): quarry-edge authenticates every crawl/scrape/event call with a
    // Bearer JWT. The onboarding website step runs BEFORE an org exists, so the
    // normal org-scoped quarry token can't be minted — this uses the dedicated
    // onboarding-preview token (sentinel org, working-set only).
    let cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let token = get_onboarding_preview_token(&state, &user.user_id, &cookie).await;
    let state_clone = state.clone();
    let output = stream! {
        let crawl_job = create_crawl_job(&state_clone, token.as_deref(), &url, max_pages).await;
        match crawl_job {
            Ok(job_id) => {
                yield Ok::<Event, std::convert::Infallible>(sse_json("started", json!({ "jobId": job_id, "url": url, "target": max_pages })));
                yield Ok(sse_json("progress", json!({ "status": "starting", "pages": 0, "elements": 0, "target": max_pages, "jobId": job_id })));
                // One ledger across seed + live: the seed scrape and the crawl
                // job both emit the homepage (page_fetched, then page_extracted),
                // and every page arrives twice (pre-/post-transform). The ledger
                // forwards the first snippet per URL, forwards a richer one as an
                // update (same id → the wizard replaces the card), and drops the
                // rest. `pages` counts unique pages, seed included.
                let mut ledger = SnippetLedger::default();
                let mut pages = 0u32;
                let mut elements = 0u32;
                let mut seed_yielded = false;
                match forward_seed_scrape(&state_clone, token.as_deref(), &url).await {
                    Ok(payloads) => {
                        for payload in payloads {
                            let Some(value) = payload.value else { continue };
                            if payload.kind == "snippet" {
                                match ledger.admit(&value) {
                                    Admission::First => {
                                        pages = pages.saturating_add(1);
                                        elements = elements.saturating_add(value.get("elementCount").and_then(Value::as_u64).unwrap_or(0) as u32);
                                    }
                                    Admission::Richer => {}
                                    Admission::Skip => continue,
                                }
                            }
                            seed_yielded = true;
                            yield Ok::<Event, std::convert::Infallible>(sse_json(&payload.kind, value));
                        }
                    }
                    Err(error) => {
                        error!(?error, "seed scrape failed");
                    }
                }
                // Stream events live: a background task polls quarry-control and pushes
                // each normalized event through the channel, which we forward the moment
                // it arrives (instead of buffering the whole crawl).
                let (tx, mut rx) =
                    tokio::sync::mpsc::channel::<crate::onboarding::crawl_preview::types::CrawlPayload>(64);
                let poll_handle = tokio::spawn({
                    let poll_state = state_clone.clone();
                    let poll_job = job_id.clone();
                    let poll_token = token.clone();
                    async move { poll_crawl_events(&poll_state, poll_token.as_deref(), &poll_job, tx).await }
                });

                while let Some(payload) = rx.recv().await {
                    match payload.kind.as_str() {
                        "snippet" => {
                            if let Some(value) = payload.value {
                                match ledger.admit(&value) {
                                    Admission::First => {
                                        pages = pages.saturating_add(1);
                                        elements = elements.saturating_add(value.get("elementCount").and_then(Value::as_u64).unwrap_or(0) as u32);
                                    }
                                    Admission::Richer => {}
                                    Admission::Skip => continue,
                                }
                                yield Ok::<Event, std::convert::Infallible>(sse_json("snippet", value));
                            }
                        }
                        "branding" => {
                            if let Some(value) = payload.value {
                                yield Ok::<Event, std::convert::Infallible>(sse_json("branding", value));
                            }
                        }
                        "progress" => {
                            if let Some(value) = payload.value {
                                yield Ok::<Event, std::convert::Infallible>(sse_json("progress", value));
                            }
                        }
                        "warning" => {
                            if let Some(value) = payload.value {
                                yield Ok::<Event, std::convert::Infallible>(sse_json("warning", value));
                            }
                        }
                        _ => {}
                    }
                }

                match poll_handle.await {
                    Ok(Ok(())) => {
                        // A run that fetched nothing and seeded nothing looks
                        // "completed" to the workflow but collected zero
                        // content — say so instead of a bare empty result.
                        if pages == 0 && !seed_yielded {
                            yield Ok::<Event, std::convert::Infallible>(sse_json("warning", json!({
                                "code": "empty_crawl",
                                "message": "Vi fikk ikke hentet innhold fra nettsiden. Prøv på nytt, eller fortsett uten forhåndsvisning."
                            })));
                        }
                        yield Ok::<Event, std::convert::Infallible>(sse_json("done", json!({
                            "count": pages,
                            "pages": pages,
                            "elements": elements,
                            "status": "completed"
                        })));
                    }
                    poll_result => {
                        if let Ok(Err(error)) = &poll_result {
                            error!(?error, "crawl event polling failed");
                        } else if poll_result.is_err() {
                            error!("crawl event polling task failed to join");
                        }
                        yield Ok::<Event, std::convert::Infallible>(sse_json("warning", json!({ "code": "control_unreachable", "message": "Could not read crawl progress." })));
                        yield Ok(sse_json("done", json!({ "count": 0, "pages": pages, "elements": elements, "status": "failed" })));
                    }
                }
            }
            Err(error) => {
                error!(?error, "crawl job create failed");
                yield Ok::<Event, std::convert::Infallible>(sse_json("warning", json!({ "code": "control_unreachable", "message": "Could not start crawl." })));
                yield Ok(sse_json("done", json!({ "count": 0, "pages": 0, "elements": 0, "status": "failed" })));
            }
        }
    };

    Sse::new(output)
        .keep_alive(KeepAlive::default())
        .into_response()
}
