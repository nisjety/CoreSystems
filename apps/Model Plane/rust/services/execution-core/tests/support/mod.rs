//! Hermetic harness for the browser planner (WS11.S0): a scripted planner and a
//! fake Quarry that the REAL `QuarryAgentClient` talks to over loopback HTTP.
//! No network beyond `127.0.0.1`, no Chromium, no inference-core.
//!
//! Not every test file uses every helper.
#![allow(dead_code)]

pub mod fake_quarry;
pub mod scripted_planner;

use execution_core::browser_agent::{
    ActionType, BrowserAction, BrowserTargetRef, PlanConfig,
};

/// A plan the loop accepts: one allowed domain, no approval, no ZDR.
pub fn plan_config() -> PlanConfig {
    PlanConfig {
        plan_id: "plan_h".to_owned(),
        grant_id: "grant_h".to_owned(),
        run_id: "run_h".to_owned(),
        org_id: "org_h".to_owned(),
        system_prompt: "Find the opening hours.".to_owned(),
        max_steps: 8,
        max_runtime_s: 60,
        allowed_domains: vec!["example.com".to_owned()],
        stop_criteria: String::new(),
        require_approval: false,
        max_cost_usd: None,
        zdr: false,
        profile_id: None,
        start_url: None,
        postcondition: String::new(),
        ..Default::default()
    }
}

/// A planner action. `action_id` and `grant_id` are filled in by the loop.
pub fn action(action_type: ActionType) -> BrowserAction {
    BrowserAction {
        action_id: String::new(),
        grant_id: String::new(),
        action_type,
        selector: String::new(),
        value: String::new(),
        url: String::new(),
        max_wait_ms: 5000,
        target: None,
        reason: String::new(),
        risk_category: None,
    }
}

pub fn goto(url: &str) -> BrowserAction {
    BrowserAction {
        url: url.to_owned(),
        ..action(ActionType::Goto)
    }
}

pub fn click_ref(snapshot_id: &str, generation: u32, ref_id: &str) -> BrowserAction {
    BrowserAction {
        target: Some(BrowserTargetRef {
            snapshot_id: snapshot_id.to_owned(),
            generation,
            ref_id: ref_id.to_owned(),
            frame_id: None,
        }),
        ..action(ActionType::Click)
    }
}

pub fn bound(mut action: BrowserAction, snapshot_id: &str, generation: u32, ref_id: &str) -> BrowserAction {
    action.target = Some(BrowserTargetRef {
        snapshot_id: snapshot_id.to_owned(),
        generation,
        ref_id: ref_id.to_owned(),
        frame_id: None,
    });
    action
}

pub fn type_ref(snapshot_id: &str, generation: u32, ref_id: &str, text: &str) -> BrowserAction {
    BrowserAction {
        value: text.to_owned(),
        ..bound(action(ActionType::Type), snapshot_id, generation, ref_id)
    }
}

pub fn select_ref(snapshot_id: &str, generation: u32, ref_id: &str, value: &str) -> BrowserAction {
    BrowserAction {
        value: value.to_owned(),
        ..bound(action(ActionType::Select), snapshot_id, generation, ref_id)
    }
}

pub fn wait_for_ref(snapshot_id: &str, generation: u32, ref_id: &str, timeout_ms: i32) -> BrowserAction {
    BrowserAction {
        max_wait_ms: timeout_ms,
        ..bound(action(ActionType::WaitFor), snapshot_id, generation, ref_id)
    }
}

pub fn press(key: &str) -> BrowserAction {
    BrowserAction {
        value: key.to_owned(),
        ..action(ActionType::Press)
    }
}

pub fn back() -> BrowserAction {
    action(ActionType::Back)
}
