//! Run + domain policy. Mirrors CONTRACTS §7.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunPolicy {
    pub concurrency: Concurrency,
    pub delay: Delay,
    pub retry: Retry,
    pub proxy: Proxy,
    pub robots: RobotsMode,
    pub ordering: Ordering,
    pub block: BlockPolicy,
    pub checkpoint: Checkpoint,
    pub determinism: Determinism,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Concurrency {
    pub per_run: u32,
    pub per_domain: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delay {
    pub min_ms: u32,
    pub max_ms: u32,
    pub jitter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Retry {
    pub max: u32,
    pub backoff: BackoffKind,
    pub base_ms: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackoffKind {
    Fixed,
    Linear,
    Exp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proxy {
    pub strategy: ProxyStrategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticky_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyStrategy {
    Rotate,
    Sticky,
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RobotsMode {
    Strict,
    Respect,
    Ignore,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ordering {
    Fifo,
    Priority,
    Lifo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockPolicy {
    pub on: BlockTrigger,
    pub action: BlockAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockTrigger {
    Challenge,
    Status429,
    Status403,
    SuspectedBot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockAction {
    Escalate,
    Retry,
    Abort,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every_n_pages: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every_s: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Determinism {
    Strict,
    BestEffort,
    Off,
}

impl Default for RunPolicy {
    fn default() -> Self {
        Self {
            concurrency: Concurrency {
                per_run: 8,
                per_domain: 2,
            },
            delay: Delay {
                min_ms: 500,
                max_ms: 2000,
                jitter: true,
            },
            retry: Retry {
                max: 3,
                backoff: BackoffKind::Exp,
                base_ms: 1000,
            },
            proxy: Proxy {
                strategy: ProxyStrategy::None,
                pool: None,
                sticky_key: None,
            },
            robots: RobotsMode::Respect,
            ordering: Ordering::Fifo,
            block: BlockPolicy {
                on: BlockTrigger::Challenge,
                action: BlockAction::Escalate,
            },
            checkpoint: Checkpoint {
                every_n_pages: Some(50),
                every_s: Some(60),
            },
            determinism: Determinism::BestEffort,
        }
    }
}
