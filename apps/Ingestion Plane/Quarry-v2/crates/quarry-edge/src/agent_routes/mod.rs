//! P7 · Phase B — agentic browser endpoint.
//!
//! Exposes Quarry's existing per-action browser machinery (`ObservationRunner`
//! + a real `BrowserDriver`) to Model Plane, which owns the planning loop:
//!
//!   POST   /v1/agent/runs             → acquire a leased browser session
//!   POST   /v1/agent/runs/{run_id}/step → execute ONE `AgentAction`, return observation
//!   DELETE /v1/agent/runs/{run_id}    → release the session
//!
//! A live `BrowserSession` is held per run in an in-process map keyed by run_id.
//! Each run's entry is behind a `tokio::Mutex` so concurrent `/step` calls for
//! the same run serialize (a browser tab can't do two actions at once) while
//! different runs proceed in parallel. Tenant isolation: the verified JWT
//! `Claims.org_id` owns the run; cross-org access is refused.
//!
//! The whole feature is gated behind `browser-agent` (on by default), which
//! pulls in the real chromiumoxide CDP driver via `quarry-browser/chromiumoxide`.

// Every handler and helper here returns `Result<_, ApiErr>`: the refusal IS the response (a
// status and a typed envelope), built on the cold path only, so the size of the `Err` variant
// costs nothing worth boxing.
#![allow(clippy::result_large_err)]

// The route handlers, request types and helpers live in the sibling files. This file keeps
// the feature gate, the imports they share (each starts with `use super::*;`) and the
// re-exports that give `crate::agent_routes::*` the paths it had before the split (WS2.B).

#[cfg(feature = "browser-agent")]
mod types;
#[cfg(feature = "browser-agent")]
mod run_live;
#[cfg(feature = "browser-agent")]
mod run_registry;
#[cfg(feature = "browser-agent")]
mod run_sweeper;
#[cfg(feature = "browser-agent")]
mod admission;
#[cfg(feature = "browser-agent")]
mod sensitive_approval;
#[cfg(feature = "browser-agent")]
mod budget;

pub(crate) mod human_browse;
#[cfg(feature = "browser-agent")]
mod control;
#[cfg(feature = "browser-agent")]
mod projection;
#[cfg(feature = "browser-agent")]
mod lifecycle;
#[cfg(feature = "browser-agent")]
mod api_key_runs;
#[cfg(feature = "browser-agent")]
mod close;
#[cfg(feature = "browser-agent")]
mod queries;
#[cfg(feature = "browser-agent")]
mod step;
#[cfg(feature = "browser-agent")]
mod receipts;
#[cfg(feature = "browser-agent")]
mod downloads;
#[cfg(feature = "browser-agent")]
mod procedure_draft;
#[cfg(feature = "browser-agent")]
mod live;
#[cfg(feature = "browser-agent")]
mod hit_test;
#[cfg(feature = "browser-agent")]
mod ws;
#[cfg(feature = "browser-agent")]
mod tabs;
#[cfg(feature = "browser-agent")]
mod tab_ledger;
#[cfg(feature = "browser-agent")]
mod attach;
#[cfg(feature = "browser-agent")]
mod origin_extension;
#[cfg(feature = "browser-agent")]
mod router;
#[cfg(all(test, feature = "browser-agent"))]
mod tests;

#[cfg(feature = "browser-agent")]
pub use self::types::*;
#[cfg(feature = "browser-agent")]
pub use self::run_live::*;
#[cfg(feature = "browser-agent")]
pub use self::run_registry::*;
#[cfg(feature = "browser-agent")]
pub use self::run_sweeper::*;
#[cfg(feature = "browser-agent")]
use self::admission::*;
#[cfg(feature = "browser-agent")]
use self::sensitive_approval::*;
#[cfg(feature = "browser-agent")]
use self::budget::*;
#[cfg(feature = "browser-agent")]
pub use self::control::*;
#[cfg(feature = "browser-agent")]
pub use self::projection::*;
#[cfg(feature = "browser-agent")]
pub use self::lifecycle::*;
#[cfg(feature = "browser-agent")]
pub use self::close::*;
#[cfg(feature = "browser-agent")]
pub use self::queries::*;
#[cfg(feature = "browser-agent")]
pub use self::step::*;
#[cfg(feature = "browser-agent")]
pub use self::receipts::*;
#[cfg(feature = "browser-agent")]
pub use self::downloads::*;
#[cfg(feature = "browser-agent")]
pub use self::procedure_draft::*;
#[cfg(feature = "browser-agent")]
pub use self::live::*;
#[cfg(feature = "browser-agent")]
pub use self::hit_test::*;
#[cfg(feature = "browser-agent")]
pub use self::ws::*;
#[cfg(feature = "browser-agent")]
pub use self::tabs::*;
#[cfg(feature = "browser-agent")]
use self::tab_ledger::*;
#[cfg(feature = "browser-agent")]
use self::attach::*;
#[cfg(feature = "browser-agent")]
pub use self::origin_extension::*;
#[cfg(feature = "browser-agent")]
pub use self::router::*;

#[cfg(feature = "browser-agent")]
use std::collections::{HashMap, HashSet};
#[cfg(feature = "browser-agent")]
use std::convert::Infallible;
#[cfg(feature = "browser-agent")]
use std::fmt::Display;
#[cfg(feature = "browser-agent")]
use std::str::FromStr;
#[cfg(feature = "browser-agent")]
use std::sync::{Arc, Mutex as StdMutex};
#[cfg(feature = "browser-agent")]
use std::time::Duration;

#[cfg(feature = "browser-agent")]
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
#[cfg(feature = "browser-agent")]
use axum::extract::{Path, Query, State};
#[cfg(feature = "browser-agent")]
use axum::http::StatusCode;
#[cfg(feature = "browser-agent")]
use axum::response::sse::{Event, KeepAlive};
#[cfg(feature = "browser-agent")]
use axum::response::{IntoResponse, Sse};
#[cfg(feature = "browser-agent")]
use axum::routing::{delete, get, post};
#[cfg(feature = "browser-agent")]
use axum::{Extension, Json, Router};
#[cfg(feature = "browser-agent")]
use serde::de::Error as _;
#[cfg(feature = "browser-agent")]
use serde::{Deserialize, Deserializer, Serialize};
#[cfg(feature = "browser-agent")]
use tokio::sync::Mutex as TokioMutex;

#[cfg(feature = "browser-agent")]
#[cfg(test)]
use quarry_browser::SessionInner;
#[cfg(feature = "browser-agent")]
use quarry_browser::{
    BrowserDevtoolsEvent, BrowserDriverCapabilities, BrowserEgressPolicy, BrowserSession,
    BrowserTab, LiveFrameFormat, LiveFrameOptions,
};
#[cfg(feature = "browser-agent")]
use quarry_core::contracts::{
    AgentAction, AgentActionRequest, AgentConstraints, BrowserEgressReceipt,
    BrowserObservation, BrowserTelemetry, ExtractionProfile,
};
#[cfg(feature = "browser-agent")]
use quarry_core::envelope::Envelope;
#[cfg(feature = "browser-agent")]
use quarry_core::error::{ErrorCode, QuarryError};
#[cfg(feature = "browser-agent")]
use quarry_core::event::EventType;
#[cfg(feature = "browser-agent")]
use quarry_core::ids::kinds::{LeaseKind, ProfileKind, RunKind};
#[cfg(feature = "browser-agent")]
use quarry_core::ids::Id;
#[cfg(feature = "browser-agent")]
use quarry_core::lease::{BrowserLease, BrowserViewport, Capability};
#[cfg(feature = "browser-agent")]
use quarry_core::privacy::PrivacyPolicy;
#[cfg(feature = "browser-agent")]
use quarry_core::zdr::ZdrMode;
#[cfg(feature = "browser-agent")]
use quarry_core::QuarryResult;
#[cfg(feature = "browser-agent")]
use quarry_runtime::browser_procedure::{
    analyze_impact, assess_quality, compare_replay, compile_procedure, BrowserProcedure,
    ProcedureImpactReport, ProcedureQualityReport, ReplayDecision,
};
#[cfg(feature = "browser-agent")]
use quarry_runtime::observation::{ObservationContext, ObservationRunner, ObservationSnapshot};
#[cfg(feature = "browser-agent")]
use quarry_runtime::step_receipts::{
    AgentIdentity, AgentIdentityBasis, AgentRunCheckpoint, AgentRunStatus, BrowserControlMode,
    BrowserProfileScope,
    BrowserAttachState, BrowserTabHolder, BrowserTabOperation, BrowserTimelineEvent,
    BrowserTimelineEventKind,
    BrowserTimelineInitiator, BrowserTimelineLifecycle, ReceiptBuilder, StepOutcome,
    StepReceipt,
};
#[cfg(feature = "browser-agent")]
use quarry_security::{Decision, SecurityEngine};
#[cfg(feature = "browser-agent")]
use url::Url;

#[cfg(feature = "browser-agent")]
use crate::state::AppState;

/// No-op router when the `browser-agent` feature is disabled, so `routes.rs`
/// can unconditionally `.merge(crate::agent_routes::agent_router())`.
#[cfg(not(feature = "browser-agent"))]
pub fn agent_router() -> axum::Router<crate::state::AppState> {
    axum::Router::new()
}

