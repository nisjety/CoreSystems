use super::*;

pub(super) struct SensitiveApproval {
    pub(super) grant_id: String,
    pub(super) parent_grant_id: String,
    pub(super) action: &'static str,
    pub(super) frame_id: Option<String>,
    pub(super) dialog_id: Option<String>,
    pub(super) artifact_id: Option<String>,
}

pub(super) fn sensitive_approval_for_action(
    entry: &RunEntry,
    action: &AgentAction,
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
            let dialog = entry
                .last_observation
                .as_ref()
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
            let target = entry
                .last_observation
                .as_ref()
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
            let target = entry
                .last_observation
                .as_ref()
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
    approval: &SensitiveApproval,
) -> QuarryResult<()> {
    let grant = state.grant_validator.validate(&approval.grant_id).await?;
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
