use super::*;

pub(super) struct SensitiveApproval {
    pub(super) grant_id: String,
    pub(super) parent_grant_id: String,
    pub(super) action: &'static str,
    pub(super) frame_id: Option<String>,
    pub(super) dialog_id: Option<String>,
    pub(super) artifact_id: Option<String>,
}

/// E3.1-b (ADR-2a): the flag that admits `fill_otp_ref`. Default off: while it is off the
/// edge refuses the action with 403 before any broker call or single-use bookkeeping.
pub(super) const FILL_OTP_FLAG: &str = "QUARRY_EDGE__FILL_OTP";

/// The sensitive-action name a BrowserBroker approval grant carries for a one-time-code
/// fill (the broker's `allowedSensitiveActions` entry; Model Plane ask in MP-ASKS.md).
pub(super) const FILL_OTP_SENSITIVE_ACTION: &str = "fill_otp";

/// Whether `QUARRY_EDGE__FILL_OTP` is on in this process. Read per call so the flag needs
/// no AppState plumbing; tests use [`fill_otp_approval`] with an explicit value instead.
pub(super) fn fill_otp_enabled() -> bool {
    std::env::var(FILL_OTP_FLAG)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// E3.1-b: the approval a `fill_otp_ref` needs. Refused 403 while the flag is off; then the
/// parent run grant and an exact target in the latest snapshot, like an upload. The
/// approval names `fill_otp` and the target's frame, and is single use: admission inserts
/// its grant id in `used_sensitive_approval_grants` like every other sensitive action.
/// The code itself never passes through here.
pub(super) fn fill_otp_approval(
    enabled: bool,
    parent_grant_id: Option<String>,
    snapshot: Option<&quarry_core::contracts::BrowserSnapshot>,
    snapshot_id: &str,
    generation: u32,
    ref_id: &str,
    approval_grant_id: &str,
) -> QuarryResult<SensitiveApproval> {
    if !enabled {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "fill_otp is not enabled on this deployment (QUARRY_EDGE__FILL_OTP is off)",
        ));
    }
    let parent_grant_id = parent_grant_id.ok_or_else(|| {
        QuarryError::new(
            ErrorCode::Forbidden,
            "sensitive browser action requires a parent BrowserBroker run grant",
        )
    })?;
    let target = snapshot
        .filter(|snapshot| snapshot.snapshot_id == snapshot_id && snapshot.generation == generation)
        .and_then(|snapshot| snapshot.targets.iter().find(|target| target.ref_id == ref_id))
        .ok_or_else(|| {
            QuarryError::new(
                ErrorCode::TargetRepairRequired,
                "one-time-code target is stale or absent; observe again before filling",
            )
        })?;
    Ok(SensitiveApproval {
        grant_id: approval_grant_id.to_owned(),
        parent_grant_id,
        action: FILL_OTP_SENSITIVE_ACTION,
        frame_id: target.frame_id.clone(),
        dialog_id: None,
        artifact_id: None,
    })
}

/// Whether `action` is one that needs a sensitive approval (a dialog reply, an upload, a
/// download, a one-time-code fill). INNH-10: an API key runs none of them, so admission refuses these for a key
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
            | AgentAction::FillOtpRef { .. }
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
        AgentAction::FillOtpRef {
            snapshot_id,
            generation,
            ref_id,
            approval_grant_id,
        } => fill_otp_approval(
            fill_otp_enabled(),
            entry.grant_id.clone(),
            last_observation.and_then(|observation| observation.snapshot.as_ref()),
            snapshot_id,
            *generation,
            ref_id,
            approval_grant_id,
        )
        .map(Some),
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

#[cfg(test)]
mod fill_otp_tests {
    use super::*;
    use quarry_core::contracts::{BrowserSnapshot, SnapshotTarget};

    fn target(ref_id: &str, frame_id: Option<&str>) -> SnapshotTarget {
        SnapshotTarget {
            ref_id: ref_id.to_owned(),
            tag: "input".to_owned(),
            text: None,
            role: Some("textbox".to_owned()),
            name: Some("Verification code".to_owned()),
            placeholder: None,
            test_id: None,
            frame_id: frame_id.map(str::to_owned),
            fingerprint: None,
            click_index: None,
        }
    }

    fn snapshot() -> BrowserSnapshot {
        BrowserSnapshot {
            snapshot_id: "snap".to_owned(),
            generation: 2,
            targets: vec![target("@e1", Some("main"))],
            accessibility: None,
            frames: vec![],
        }
    }

    fn approve(enabled: bool, parent: Option<&str>, generation: u32) -> QuarryResult<SensitiveApproval> {
        let snapshot = snapshot();
        fill_otp_approval(
            enabled,
            parent.map(str::to_owned),
            Some(&snapshot),
            "snap",
            generation,
            "@e1",
            "child-grant",
        )
    }

    #[test]
    fn the_flag_off_refuses_with_403_before_anything_else() {
        // Off, even a complete request (parent grant, live target) is refused.
        let error = approve(false, Some("run-grant"), 2).err().expect("refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(error.code.http_status(), 403);
        // Off is also the answer when nothing else would pass.
        let error = fill_otp_approval(false, None, None, "x", 9, "@e9", "g").err().expect("refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
    }

    #[test]
    fn the_flag_defaults_off() {
        if std::env::var_os(FILL_OTP_FLAG).is_none() {
            assert!(!fill_otp_enabled());
        }
    }

    #[test]
    fn on_it_names_fill_otp_the_target_frame_and_the_parent_grant() {
        let approval = approve(true, Some("run-grant"), 2).expect("approval");
        assert_eq!(approval.action, "fill_otp");
        assert_eq!(approval.grant_id, "child-grant");
        assert_eq!(approval.parent_grant_id, "run-grant");
        assert_eq!(approval.frame_id.as_deref(), Some("main"));
        assert!(approval.dialog_id.is_none());
        assert!(approval.artifact_id.is_none());
    }

    #[test]
    fn on_it_still_needs_a_parent_run_grant() {
        let error = approve(true, None, 2).err().expect("refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
    }

    #[test]
    fn a_stale_generation_or_an_unknown_ref_asks_for_a_new_observation() {
        let error = approve(true, Some("run-grant"), 1).err().expect("stale");
        assert_eq!(error.code, ErrorCode::TargetRepairRequired);
        let snapshot = snapshot();
        let error = fill_otp_approval(true, Some("run-grant".into()), Some(&snapshot), "snap", 2, "@e7", "g")
            .err()
            .expect("unknown ref");
        assert_eq!(error.code, ErrorCode::TargetRepairRequired);
        let error = fill_otp_approval(true, Some("run-grant".into()), None, "snap", 2, "@e1", "g")
            .err()
            .expect("no snapshot");
        assert_eq!(error.code, ErrorCode::TargetRepairRequired);
    }

    #[test]
    fn a_fill_otp_ref_is_a_sensitive_action() {
        let action = AgentAction::FillOtpRef {
            snapshot_id: "snap".into(),
            generation: 2,
            ref_id: "@e1".into(),
            approval_grant_id: "g".into(),
        };
        assert!(is_sensitive_action(&action));
    }
}
