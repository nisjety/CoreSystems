//! Action sequencing. Inspired by Firecrawl/ScrapingBee `actions` arrays.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Wait {
        ms: u32,
    },
    WaitFor {
        selector: String,
        timeout_ms: u32,
    },
    Click {
        selector: String,
    },
    ClickPoint {
        x: f64,
        y: f64,
    },
    Type {
        selector: String,
        text: String,
    },
    Scroll {
        to: ScrollTarget,
    },
    MouseWheel {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    },
    Screenshot {
        full_page: bool,
    },
    Pdf,
    Evaluate {
        script: String,
    },
    Navigate {
        url: String,
    },
    Press {
        key: String,
    },
    Select {
        selector: String,
        value: String,
    },
    Back,
    Forward,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollTarget {
    Top,
    Bottom,
    Selector(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionScript {
    pub actions: Vec<Action>,
    #[serde(default)]
    pub on_error: OnError,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnError {
    #[default]
    Abort,
    Continue,
    Retry,
}
