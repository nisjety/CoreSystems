//! Browser preview benchmark for the Codex-grade in-app browser path.
//!
//! This intentionally uses Quarry's real `BrowserDriver` and `ArtifactStore`
//! traits with deterministic mock implementations. CI can run it without
//! Chrome, while the report still measures the contract-level difference
//! between:
//!
//! - legacy preview refresh: screenshot capture + durable artifact write
//! - live frame preview: transient `live_frame` capture with no artifact write
//!
//! Browser actions still write one observation artifact in both scenarios so
//! replay/debug evidence remains deterministic outside ZDR.

use std::{
    collections::BTreeMap,
    env,
    error::Error,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bytes::Bytes;
use quarry_browser::{
    actions::ScrollTarget, BrowserDriver, BrowserSession, LiveFrame, LiveFrameOptions, SessionInner,
};
use quarry_core::{
    ids::{
        kinds::{ArtifactKind, RunKind},
        Id,
    },
    lease::{BrowserLease, BrowserViewport, Capability, ProxyAffinity},
    pagination::{ListFilter, Page},
    resources::ArtifactSummary,
    ErrorCode, QuarryError, QuarryResult,
};
use quarry_runtime::artifact_store::{ArtifactHandle, ArtifactStore};
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};

const PAGE_HASH: &str = "blake3:browser-preview-benchmark";
/// Tenant stamped on benchmark artifacts. The benchmark store discards bodies,
/// so this only has to be a stable, non-production org id.
const BENCHMARK_ORG: &str = "org_benchmark";

#[derive(Debug, Clone)]
struct BenchmarkConfig {
    frames: usize,
    actions: usize,
    frame_bytes: usize,
    out: Option<PathBuf>,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            frames: 30,
            actions: 10,
            frame_bytes: 96 * 1024,
            out: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ScenarioKind {
    ScreenshotArtifactRefresh,
    LiveFrameStream,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkReport {
    benchmark: &'static str,
    quarry_version: &'static str,
    config: ReportConfig,
    scenarios: Vec<ScenarioReport>,
    comparison: BenchmarkComparison,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportConfig {
    frames: usize,
    actions: usize,
    frame_bytes: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScenarioReport {
    scenario: ScenarioKind,
    frame_latency_ms: LatencyStats,
    action_latency_ms: LatencyStats,
    cpu_ms: f64,
    artifact_write_count: u64,
    artifact_bytes: u64,
    artifact_writes_by_kind: BTreeMap<String, u64>,
    persisted_preview_image_payloads: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkComparison {
    artifact_writes_saved: i64,
    artifact_bytes_saved: i64,
    persisted_preview_image_payloads_saved: i64,
    frame_p95_delta_ms: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct LatencyStats {
    min: f64,
    mean: f64,
    p50: f64,
    p95: f64,
    max: f64,
}

#[derive(Default)]
struct CountingArtifactStore {
    writes: AtomicU64,
    bytes: AtomicU64,
    by_kind: RwLock<BTreeMap<String, u64>>,
}

impl CountingArtifactStore {
    async fn snapshot(&self) -> ArtifactSnapshot {
        ArtifactSnapshot {
            writes: self.writes.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            by_kind: self.by_kind.read().await.clone(),
        }
    }
}

struct ArtifactSnapshot {
    writes: u64,
    bytes: u64,
    by_kind: BTreeMap<String, u64>,
}

#[async_trait]
impl ArtifactStore for CountingArtifactStore {
    async fn put(
        &self,
        org_id: &str,
        run_id: &RunKind,
        page_hash: &str,
        kind: &str,
        body: Vec<u8>,
    ) -> QuarryResult<ArtifactHandle> {
        let id: ArtifactKind = Id::new();
        let bytes = body.len() as u64;
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        *self
            .by_kind
            .write()
            .await
            .entry(kind.to_owned())
            .or_default() += 1;

        Ok(ArtifactHandle {
            artifact_id: id,
            key: format!("org={org_id}/{run_id}/{page_hash}/{kind}"),
            bytes,
        })
    }

    async fn get(&self, _org_id: &str, id: &ArtifactKind) -> QuarryResult<Vec<u8>> {
        Err(QuarryError::new(
            ErrorCode::NotFound,
            format!("artifact {id} is not retained by the benchmark store"),
        ))
    }

    async fn list(
        &self,
        _org_id: &str,
        _filter: &ListFilter,
    ) -> QuarryResult<Page<ArtifactSummary>> {
        Ok(Page::empty())
    }

    async fn count(&self, _org_id: &str) -> QuarryResult<Option<u64>> {
        Ok(Some(self.writes.load(Ordering::Relaxed)))
    }
}

struct DeterministicBrowserDriver {
    screenshot_bytes: Vec<u8>,
    live_frame_bytes: Vec<u8>,
    screenshot_work: u32,
    live_frame_work: u32,
    action_work: u32,
}

impl DeterministicBrowserDriver {
    fn new(frame_bytes: usize) -> Self {
        Self {
            screenshot_bytes: deterministic_bytes(frame_bytes, 0x51),
            live_frame_bytes: deterministic_bytes(frame_bytes / 2, 0x17),
            screenshot_work: 260,
            live_frame_work: 75,
            action_work: 95,
        }
    }
}

#[async_trait]
impl BrowserDriver for DeterministicBrowserDriver {
    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        Ok(BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(Mutex::new(SessionInner {
                connected: true,
                pages_served: 0,
            })),
        })
    }

    async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
        Ok(())
    }

    async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
        burn_cpu(self.action_work);
        Ok(())
    }

    async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        Ok(Bytes::from_static(
            b"<html><head><title>Benchmark</title></head><body>ok</body></html>",
        ))
    }

    async fn screenshot(&self, _session: &BrowserSession, _full_page: bool) -> QuarryResult<Bytes> {
        burn_cpu(self.screenshot_work);
        Ok(Bytes::copy_from_slice(&self.screenshot_bytes))
    }

    async fn live_frame(
        &self,
        _session: &BrowserSession,
        _options: LiveFrameOptions,
    ) -> QuarryResult<LiveFrame> {
        burn_cpu(self.live_frame_work);
        Ok(LiveFrame {
            mime_type: "image/jpeg".to_owned(),
            data_base64: BASE64_STANDARD.encode(&self.live_frame_bytes),
        })
    }

    async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        Ok(Bytes::new())
    }

    async fn click_point(&self, _session: &BrowserSession, _x: f64, _y: f64) -> QuarryResult<()> {
        burn_cpu(self.action_work);
        Ok(())
    }

    async fn mouse_wheel(
        &self,
        _session: &BrowserSession,
        _x: f64,
        _y: f64,
        _delta_x: f64,
        _delta_y: f64,
    ) -> QuarryResult<()> {
        burn_cpu(self.action_work);
        Ok(())
    }

    async fn press(&self, _session: &BrowserSession, _key: &str) -> QuarryResult<()> {
        burn_cpu(self.action_work);
        Ok(())
    }

    async fn scroll(&self, _session: &BrowserSession, _target: &ScrollTarget) -> QuarryResult<()> {
        burn_cpu(self.action_work);
        Ok(())
    }

    async fn evaluate(
        &self,
        _session: &BrowserSession,
        _script: &str,
    ) -> QuarryResult<serde_json::Value> {
        Ok(serde_json::json!({
            "url": "https://benchmark.local/",
            "title": "Benchmark"
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_args(env::args().skip(1))?;
    let report = run_benchmark(config.clone()).await?;
    let json = serde_json::to_string_pretty(&report)?;

    if let Some(path) = &config.out {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, format!("{json}\n"))?;
        println!("browser benchmark: wrote {}", path.display());
    } else {
        println!("{json}");
    }

    Ok(())
}

async fn run_benchmark(config: BenchmarkConfig) -> QuarryResult<BenchmarkReport> {
    let baseline = run_scenario(&config, ScenarioKind::ScreenshotArtifactRefresh).await?;
    let live = run_scenario(&config, ScenarioKind::LiveFrameStream).await?;

    let comparison = BenchmarkComparison {
        artifact_writes_saved: baseline.artifact_write_count as i64
            - live.artifact_write_count as i64,
        artifact_bytes_saved: baseline.artifact_bytes as i64 - live.artifact_bytes as i64,
        persisted_preview_image_payloads_saved: baseline.persisted_preview_image_payloads as i64
            - live.persisted_preview_image_payloads as i64,
        frame_p95_delta_ms: live.frame_latency_ms.p95 - baseline.frame_latency_ms.p95,
    };

    Ok(BenchmarkReport {
        benchmark: "quarry_browser_live_frame_stream",
        quarry_version: env!("CARGO_PKG_VERSION"),
        config: ReportConfig {
            frames: config.frames,
            actions: config.actions,
            frame_bytes: config.frame_bytes,
        },
        scenarios: vec![baseline, live],
        comparison,
    })
}

async fn run_scenario(
    config: &BenchmarkConfig,
    scenario: ScenarioKind,
) -> QuarryResult<ScenarioReport> {
    let run_id: RunKind = Id::new();
    let store = Arc::new(CountingArtifactStore::default());
    let driver = Arc::new(DeterministicBrowserDriver::new(config.frame_bytes));
    let lease = browser_lease();
    let session = driver.acquire(&lease).await?;
    let mut frame_latencies = Vec::with_capacity(config.frames);
    let mut action_latencies = Vec::with_capacity(config.actions);
    let cpu_start = process_cpu_time();
    let mut persisted_preview_image_payloads = 0_u64;

    for _ in 0..config.frames {
        let start = Instant::now();
        match scenario {
            ScenarioKind::ScreenshotArtifactRefresh => {
                let screenshot = driver.screenshot(&session, false).await?;
                store
                    .put(
                        BENCHMARK_ORG,
                        &run_id,
                        PAGE_HASH,
                        "screenshot",
                        screenshot.to_vec(),
                    )
                    .await?;
                persisted_preview_image_payloads += 1;
            }
            ScenarioKind::LiveFrameStream => {
                let _frame = driver
                    .live_frame(&session, LiveFrameOptions::default())
                    .await?;
            }
        }
        frame_latencies.push(start.elapsed());
    }

    for index in 0..config.actions {
        let start = Instant::now();
        match index % 3 {
            0 => driver.click_point(&session, 180.0, 220.0).await?,
            1 => {
                driver
                    .mouse_wheel(&session, 180.0, 220.0, 0.0, 420.0)
                    .await?
            }
            _ => driver.press(&session, "Tab").await?,
        }
        let screenshot = driver.screenshot(&session, false).await?;
        store
            .put(
                BENCHMARK_ORG,
                &run_id,
                PAGE_HASH,
                "screenshot",
                screenshot.to_vec(),
            )
            .await?;
        action_latencies.push(start.elapsed());
    }

    driver.release(session).await?;
    let cpu_ms = process_cpu_time()
        .checked_sub(cpu_start)
        .unwrap_or(Duration::ZERO)
        .as_secs_f64()
        * 1_000.0;
    let artifacts = store.snapshot().await;

    Ok(ScenarioReport {
        scenario,
        frame_latency_ms: latency_stats(&frame_latencies),
        action_latency_ms: latency_stats(&action_latencies),
        cpu_ms,
        artifact_write_count: artifacts.writes,
        artifact_bytes: artifacts.bytes,
        artifact_writes_by_kind: artifacts.by_kind,
        persisted_preview_image_payloads,
    })
}

fn browser_lease() -> BrowserLease {
    BrowserLease {
        lease_id: Id::new(),
        profile_id: Id::new(),
        session_affinity_key: "browser-preview-benchmark".to_owned(),
        proxy_affinity: ProxyAffinity {
            pool: "benchmark".to_owned(),
            sticky_key: None,
        },
        ttl_s: 60,
        capabilities: vec![Capability::Actions, Capability::Screenshots],
        artifact_bucket: "benchmark".to_owned(),
        persist_profile: false,
        viewport: Some(BrowserViewport {
            width: 1280,
            height: 800,
            device_scale_factor: 1.0,
            is_mobile: false,
        }),
        org_id: "benchmark-org".to_owned(),
    }
}

fn latency_stats(samples: &[Duration]) -> LatencyStats {
    if samples.is_empty() {
        return LatencyStats {
            min: 0.0,
            mean: 0.0,
            p50: 0.0,
            p95: 0.0,
            max: 0.0,
        };
    }

    let mut values: Vec<f64> = samples
        .iter()
        .map(|duration| duration.as_secs_f64() * 1_000.0)
        .collect();
    values.sort_by(f64::total_cmp);
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    LatencyStats {
        min: values[0],
        mean,
        p50: percentile(&values, 0.50),
        p95: percentile(&values, 0.95),
        max: *values.last().unwrap_or(&0.0),
    }
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn deterministic_bytes(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|index| seed.wrapping_add((index % 251) as u8))
        .collect()
}

fn burn_cpu(rounds: u32) {
    let mut acc = 0_u64;
    for index in 0..rounds.saturating_mul(512) {
        acc = acc
            .wrapping_add(u64::from(index).wrapping_mul(6364136223846793005))
            .rotate_left(7);
    }
    std::hint::black_box(acc);
}

fn parse_args<I>(args: I) -> Result<BenchmarkConfig, Box<dyn Error>>
where
    I: IntoIterator<Item = String>,
{
    let mut config = BenchmarkConfig::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => {
                config.frames = parse_next_usize(&mut args, "--frames")?;
            }
            "--actions" => {
                config.actions = parse_next_usize(&mut args, "--actions")?;
            }
            "--frame-bytes" => {
                config.frame_bytes = parse_next_usize(&mut args, "--frame-bytes")?;
            }
            "--out" => {
                let path = args.next().ok_or("--out requires a path")?;
                config.out = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                return Err(usage().into());
            }
            other => {
                return Err(format!("unknown argument {other}\n{}", usage()).into());
            }
        }
    }
    Ok(config)
}

fn parse_next_usize<I>(args: &mut I, flag: &str) -> Result<usize, Box<dyn Error>>
where
    I: Iterator<Item = String>,
{
    let value = args
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))?;
    let parsed = value.parse::<usize>()?;
    Ok(parsed)
}

fn usage() -> &'static str {
    "usage: quarry-browser-benchmark [--frames N] [--actions N] [--frame-bytes N] [--out PATH]"
}

#[cfg(unix)]
fn process_cpu_time() -> Duration {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if rc != 0 {
        return Duration::ZERO;
    }
    let usage = unsafe { usage.assume_init() };
    timeval_duration(usage.ru_utime) + timeval_duration(usage.ru_stime)
}

#[cfg(unix)]
fn timeval_duration(value: libc::timeval) -> Duration {
    let secs = value.tv_sec.max(0) as u64;
    let micros = value.tv_usec.max(0) as u32;
    Duration::new(secs, micros.saturating_mul(1_000))
}

#[cfg(not(unix))]
fn process_cpu_time() -> Duration {
    Duration::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_stats_handles_empty_samples() {
        let stats = latency_stats(&[]);
        assert_eq!(stats.p95, 0.0);
        assert_eq!(stats.mean, 0.0);
    }

    #[tokio::test]
    async fn live_frame_stream_saves_preview_artifact_writes() {
        let report = run_benchmark(BenchmarkConfig {
            frames: 4,
            actions: 2,
            frame_bytes: 8 * 1024,
            out: None,
        })
        .await
        .expect("benchmark report");

        let baseline = &report.scenarios[0];
        let live = &report.scenarios[1];
        assert_eq!(baseline.scenario, ScenarioKind::ScreenshotArtifactRefresh);
        assert_eq!(live.scenario, ScenarioKind::LiveFrameStream);
        assert_eq!(baseline.artifact_write_count, 6);
        assert_eq!(live.artifact_write_count, 2);
        assert_eq!(report.comparison.artifact_writes_saved, 4);
        assert_eq!(baseline.persisted_preview_image_payloads, 4);
        assert_eq!(live.persisted_preview_image_payloads, 0);
    }
}
