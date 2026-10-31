use super::*;

pub(super) struct SensitiveApproval {
    pub(super) grant_id: String,
    pub(super) parent_grant_id: String,
    pub(super) action: &'static str,
    pub(super) frame_id: Option<String>,
    pub(super) dialog_id: Option<String>,
    pub(super) artifact_id: Option<String>,
}

/// Whether `action` is one that needs a sensitive approval (a dialog reply, an upload, a
/// download). INNH-10: an API key runs none of them, so admission refuses these for a key
/// run on the action's kind alone, before the driver's capabilities or any observation are
/// consulted and before any broker call.
pub(super) fn is_sensitive_action(action: &AgentAction) -> bool {
    matches!(
        action,
        AgentAction::RespondDialog { .. }
            | AgentAction::UploadRef { .. }
            | AgentAction::FrameUploadRef { .. }
            | AgentAction::DownloadRef { .. }
            | AgentAction::FrameDownloadRef { .. }
    )
}

pub(super) fn sensitive_approval_for_action(
    entry: &RunEntry,
    action: &AgentAction,
    // WS8.H: the observation of the lane this action runs on, which the approval names.
    last_observation: Option<&BrowserObservation>,
) -> QuarryResult<Option<SensitiveApproval>> {
    match action {
        AgentAction::RespondDialog {
            dialog_id,
            approval_grant_id,
            ..
        } => {
            let parent_grant_id = entry.grant_id.clone().ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::Forbidden,
                    "sensitive browser action requires a parent BrowserBroker run grant",
                )
            })?;
            let dialog = last_observation
                .and_then(|observation| {
                    observation
                        .dialogs
                        .iter()
                        .find(|candidate| candidate.dialog_id == *dialog_id)
                })
                .ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::TargetRepairRequired,
                        "dialog is no longer observed; observe again before responding",
                    )
                })?;
            Ok(Some(SensitiveApproval {
                grant_id: approval_grant_id.clone(),
                parent_grant_id: parent_grant_id.clone(),
                action: "respond_dialog",
                frame_id: dialog.frame_id.clone(),
                dialog_id: Some(dialog_id.clone()),
                artifact_id: None,
            }))
        }
        AgentAction::UploadRef {
            snapshot_id,
            generation,
            ref_id,
            artifact_id,
            approval_grant_id,
        }
        | AgentAction::FrameUploadRef {
            snapshot_id,
            generation,
            ref_id,
            artifact_id,
            approval_grant_id,
            ..
        } => {
            let parent_grant_id = entry.grant_id.clone().ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::Forbidden,
                    "sensitive browser action requires a parent BrowserBroker run grant",
                )
            })?;
            let target = last_observation
                .and_then(|observation| observation.snapshot.as_ref())
                .filter(|snapshot| {
                    snapshot.snapshot_id == *snapshot_id && snapshot.generation == *generation
                })
                .and_then(|snapshot| {
                    snapshot
                        .targets
                        .iter()
                        .find(|target| target.ref_id == *ref_id)
                })
                .ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::TargetRepairRequired,
                        "upload target is stale or absent; observe again before uploading",
                    )
                })?;
            Ok(Some(SensitiveApproval {
                grant_id: approval_grant_id.clone(),
                parent_grant_id,
                action: "upload_ref",
                frame_id: target.frame_id.clone(),
                dialog_id: None,
                artifact_id: Some(artifact_id.to_string()),
            }))
        }
        AgentAction::DownloadRef {
            snapshot_id,
            generation,
            ref_id,
            approval_grant_id,
        }
        | AgentAction::FrameDownloadRef {
            snapshot_id,
            generation,
            ref_id,
            approval_grant_id,
            ..
        } => {
            let parent_grant_id = entry.grant_id.clone().ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::Forbidden,
                    "sensitive browser action requires a parent BrowserBroker run grant",
                )
            })?;
            let target = last_observation
                .and_then(|observation| observation.snapshot.as_ref())
                .filter(|snapshot| {
                    snapshot.snapshot_id == *snapshot_id && snapshot.generation == *generation
                })
                .and_then(|snapshot| {
                    snapshot
                        .targets
                        .iter()
                        .find(|target| target.ref_id == *ref_id)
                })
                .ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::TargetRepairRequired,
                        "download target is stale or absent; observe again before downloading",
                    )
                })?;
            Ok(Some(SensitiveApproval {
                grant_id: approval_grant_id.clone(),
                parent_grant_id,
                action: "download_ref",
                frame_id: target.frame_id.clone(),
                dialog_id: None,
                artifact_id: None,
            }))
        }
        _ => Ok(None),
    }
}

pub(super) async fn validate_sensitive_approval(
    state: &AppState,
    org_id: &str,
    approval: &SensitiveApproval,
) -> QuarryResult<()> {
    let grant = state
        .grant_validator
        .validate_for_org(org_id, &approval.grant_id)
        .await?;
    if !grant.is_usable() {
        Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            "sensitive browser approval grant is inactive or expired",
        ))
    } else if !grant.authorizes_exact_sensitive_action(
        approval.action,
        &approval.parent_grant_id,
        approval.frame_id.as_deref(),
        approval.dialog_id.as_deref(),
        approval.artifact_id.as_deref(),
    ) {
        Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            "sensitive browser approval grant does not match the observed action scope",
        ))
    } else {
        Ok(())
    }
}
