//! Bounds on text a person sends to a run while it works (mid-run input).
//!
//! One definition for every process that enforces them: model-gateway's queue
//! for chat turns (`queued_input`) and execution-core's steer queue for a live
//! browser run (E3.3-f). A steer text is held to exactly the caps a queued chat
//! message is.

/// Per message, in characters. A mid-run injection spends prompt budget the run
/// never planned for.
pub const MAX_QUEUED_CHARS: usize = 2_000;

/// Per run. Past a few, the person is not steering; they are typing at a wall,
/// and delivering all of it would bury the task the run is actually on.
pub const MAX_QUEUED_MESSAGES: usize = 3;
