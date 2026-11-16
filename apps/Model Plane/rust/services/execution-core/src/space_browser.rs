//! The Space agreement of a browser step (build plan WS20-S3).
//!
//! A room browser run is started in a Space by model-gateway after it verified a Control decision. The
//! grant it derives names no Space (C1), so the durable run is the authority for the Space a browser step
//! belongs to. Execution-core reads that Space from Session Core (`ResolveRunSpace`, after the owner
//! check) and compares it with the step's `space_id` here:
//!
//! * a step that names a Space its run was not started in is refused;
//! * a step that names no Space for a run that was started in one is refused (no silent downgrade);
//! * a personal run (no Space on either side) and a room run (the same Space on both) are admitted.

use tonic::Status;

/// Refuses a browser step whose `space_id` is not the durable run's Space. Both empty is a personal run;
/// both equal is a room run. Any other pair is a mismatch.
#[allow(clippy::result_large_err)]
pub(crate) fn check_run_space(request_space_id: &str, durable_space_id: &str) -> Result<(), Status> {
    if request_space_id == durable_space_id {
        return Ok(());
    }
    Err(Status::permission_denied(
        "browser grant does not match the run's Space",
    ))
}
