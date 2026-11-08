//! The class of each agent action, and the capability key that gates it.
//!
//! [`classify`] is an exhaustive match with no wildcard arm. A new
//! [`AgentAction`] variant is a compile error here until it is classified, so
//! no action can reach a browser without a capability decision.

use super::CapKey;
use crate::contracts::AgentAction;

/// The gate class of an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    /// Raw page script (`evaluate`). Never unlockable.
    Script,
    /// Typing, key presses and option selection.
    Input,
    /// Clicks, pointer and wheel input, scrolling and dialog replies.
    Interact,
    /// Uploading a tenant artifact into a page.
    Upload,
    /// Collecting a download into artifact storage.
    Download,
    /// Moving the page: navigate, back, forward. Gated by domain rules, not a capability.
    Navigation,
    /// Reading the page: waits, screenshots, PDF, content. Not gated by a capability.
    Observation,
}

impl ActionClass {
    /// The capability that gates this class, or `None` when only domain rules apply.
    pub fn cap_key(self) -> Option<CapKey> {
        match self {
            Self::Script => Some(CapKey::ActScript),
            Self::Input => Some(CapKey::ActInput),
            Self::Interact => Some(CapKey::ActInteract),
            Self::Upload => Some(CapKey::ActUpload),
            Self::Download => Some(CapKey::ActDownload),
            Self::Navigation | Self::Observation => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Script => "script",
            Self::Input => "input",
            Self::Interact => "interact",
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Navigation => "navigation",
            Self::Observation => "observation",
        }
    }
}

/// The gate class of `action`.
pub fn action_class(action: &AgentAction) -> ActionClass {
    classify(action).0
}

/// The action's wire tag (its serde `type`), used as `denied_action`.
pub fn action_tag(action: &AgentAction) -> &'static str {
    classify(action).1
}

/// Exhaustive over every [`AgentAction`] variant. Keep it wildcard-free.
fn classify(action: &AgentAction) -> (ActionClass, &'static str) {
    use ActionClass as C;
    match action {
        AgentAction::Navigate { .. } => (C::Navigation, "navigate"),
        AgentAction::Click { .. } => (C::Interact, "click"),
        AgentAction::ClickRef { .. } => (C::Interact, "click_ref"),
        AgentAction::FrameClickRef { .. } => (C::Interact, "frame_click_ref"),
        AgentAction::ClickSemantic { .. } => (C::Interact, "click_semantic"),
        AgentAction::ClickPoint { .. } => (C::Interact, "click_point"),
        AgentAction::Type { .. } => (C::Input, "type"),
        AgentAction::TypeRef { .. } => (C::Input, "type_ref"),
        AgentAction::FrameTypeRef { .. } => (C::Input, "frame_type_ref"),
        AgentAction::TypeSemantic { .. } => (C::Input, "type_semantic"),
        AgentAction::Press { .. } => (C::Input, "press"),
        AgentAction::Scroll { .. } => (C::Interact, "scroll"),
        AgentAction::MouseWheel { .. } => (C::Interact, "mouse_wheel"),
        AgentAction::Select { .. } => (C::Input, "select"),
        AgentAction::SelectRef { .. } => (C::Input, "select_ref"),
        AgentAction::FrameSelectRef { .. } => (C::Input, "frame_select_ref"),
        AgentAction::SelectSemantic { .. } => (C::Input, "select_semantic"),
        AgentAction::Wait { .. } => (C::Observation, "wait"),
        AgentAction::WaitFor { .. } => (C::Observation, "wait_for"),
        AgentAction::WaitForRef { .. } => (C::Observation, "wait_for_ref"),
        AgentAction::FrameWaitForRef { .. } => (C::Observation, "frame_wait_for_ref"),
        AgentAction::WaitForSemantic { .. } => (C::Observation, "wait_for_semantic"),
        AgentAction::RespondDialog { .. } => (C::Interact, "respond_dialog"),
        AgentAction::UploadRef { .. } => (C::Upload, "upload_ref"),
        AgentAction::FrameUploadRef { .. } => (C::Upload, "frame_upload_ref"),
        AgentAction::DownloadRef { .. } => (C::Download, "download_ref"),
        AgentAction::FrameDownloadRef { .. } => (C::Download, "frame_download_ref"),
        // E3.1-a: typing a one-time code is an input action on the page.
        AgentAction::FillOtpRef { .. } => (C::Input, "fill_otp_ref"),
        AgentAction::Screenshot { .. } => (C::Observation, "screenshot"),
        AgentAction::Pdf => (C::Observation, "pdf"),
        AgentAction::Evaluate { .. } => (C::Script, "evaluate"),
        AgentAction::Back => (C::Navigation, "back"),
        AgentAction::Forward => (C::Navigation, "forward"),
        AgentAction::Reload { .. } => (C::Navigation, "reload"),
        AgentAction::GetContent => (C::Observation, "get_content"),
    }
}
