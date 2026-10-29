//! Scriptable `FetchAdapter` so tests can dictate exactly what each fetch
//! returns or how it fails, and inspect what the executor asked for.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use tokio::sync::Notify;
use vsv2_common::{Error, PageSink, Result, TenantId};
use vsv2_orchestrator::{
    EndpointDefinition, FetchAdapter, FetchPartitionResult, FetchWindow, PartitionPlan,
};

/// How a scripted fetch fails.
#[derive(Debug, Clone, Copy)]
pub enum Failure {
    Upstream(u16),
    RateLimited,
    Other,
    /// An upstream error whose body is binary junk, NUL bytes included.
    BinaryBody,
}

impl Failure {
    fn into_error(self) -> Error {
        match self {
            Failure::Upstream(status) => Error::UpstreamStatus {
                status,
                body: "scripted failure".into(),
            },
            Failure::RateLimited => Error::RateLimited {
                kind: "erp_rest",
                until: Utc::now() + Duration::seconds(30),
            },
            Failure::Other => Error::Other("scripted failure".into()),
            Failure::BinaryBody => Error::UpstreamStatus {
                status: 502,
                body: "\0\0\u{1f}binary \0junk".into(),
            },
        }
    }
}

/// Holds one fetch open so a test can run another sync while it is in flight.
///
/// The fetch signals `entered` and then waits for `release`. `Notify` stores a
/// permit, so neither side races the other.
#[derive(Debug, Clone, Default)]
pub struct Gate {
    pub entered: Arc<Notify>,
    pub release: Arc<Notify>,
}

/// One scripted response to a `fetch_partition` call.
#[derive(Debug, Clone)]
pub enum Step {
    Records(Vec<Value>),
    /// Returns the records only after the test opens the gate.
    Gated(Gate, Vec<Value>),
    Fail(Failure),
}

/// What the executor passed to `fetch_partition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchCall {
    pub partition_key: String,
    pub watermark: Option<DateTime<Utc>>,
    pub window_end: Option<DateTime<Utc>>,
    pub cursor: Option<String>,
}

pub struct FakeAdapter {
    partitions: Vec<PartitionPlan>,
    script: Mutex<HashMap<String, VecDeque<Step>>>,
    calls: Mutex<Vec<FetchCall>>,
}

impl FakeAdapter {
    /// An endpoint with the single `default` partition.
    pub fn single_partition() -> Arc<Self> {
        Self::with_partitions(&[vsv2_state::DEFAULT_PARTITION_KEY])
    }

    pub fn with_partitions(keys: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            partitions: keys
                .iter()
                .map(|key| PartitionPlan {
                    key: (*key).to_string(),
                    ..Default::default()
                })
                .collect(),
            script: Mutex::new(HashMap::new()),
            calls: Mutex::new(Vec::new()),
        })
    }

    /// Queue the response for the next unanswered fetch of `partition_key`.
    pub fn push(&self, partition_key: &str, step: Step) {
        self.script
            .lock()
            .expect("script lock")
            .entry(partition_key.to_string())
            .or_default()
            .push_back(step);
    }

    pub fn calls(&self) -> Vec<FetchCall> {
        self.calls.lock().expect("calls lock").clone()
    }
}

/// `DispatchTable` wants a `Box<dyn FetchAdapter>`; tests keep their own `Arc`
/// to script and inspect the adapter, so this newtype forwards to it.
pub struct SharedAdapter(pub Arc<FakeAdapter>);

#[async_trait]
impl FetchAdapter for SharedAdapter {
    async fn plan_partitions(
        &self,
        _tenant: &TenantId,
        _def: &EndpointDefinition,
    ) -> Result<Vec<PartitionPlan>> {
        Ok(self.0.partitions.clone())
    }

    async fn fetch_partition(
        &self,
        _tenant: &TenantId,
        def: &EndpointDefinition,
        partition: &PartitionPlan,
        window: FetchWindow,
        sink: &mut dyn PageSink,
    ) -> Result<FetchPartitionResult> {
        self.0.calls.lock().expect("calls lock").push(FetchCall {
            partition_key: partition.key.clone(),
            watermark: window.watermark,
            window_end: window.window_end,
            cursor: window.cursor,
        });

        let step = self
            .0
            .script
            .lock()
            .expect("script lock")
            .get_mut(&partition.key)
            .and_then(VecDeque::pop_front);

        match step {
            Some(Step::Records(records)) => deliver(def, partition, records, sink),
            Some(Step::Gated(gate, records)) => {
                gate.entered.notify_one();
                gate.release.notified().await;
                deliver(def, partition, records, sink)
            }
            Some(Step::Fail(failure)) => Err(failure.into_error()),
            None => Err(Error::Other(format!(
                "FakeAdapter: no scripted step for partition '{}'",
                partition.key
            ))),
        }
    }
}

fn deliver(
    def: &EndpointDefinition,
    partition: &PartitionPlan,
    mut records: Vec<Value>,
    sink: &mut dyn PageSink,
) -> Result<FetchPartitionResult> {
    // The real ERP adapter stamps the partition onto every row.
    if def.partitioning.is_some() || !partition.is_default() {
        for record in &mut records {
            if let Value::Object(obj) = record {
                obj.insert(
                    "_partition_key".into(),
                    Value::String(partition.key.clone()),
                );
            }
        }
    }
    sink.accept(records)?;
    Ok(FetchPartitionResult {
        page_metrics: vec![],
    })
}
