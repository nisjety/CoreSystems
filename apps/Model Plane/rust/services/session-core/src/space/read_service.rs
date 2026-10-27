//! The empty `SpaceRoomReadService` (build plan P-SC1, decision D3).
//!
//! The service is registered with no RPC so that WS07-S9, WS15-S6 and WS11-S2
//! each add their RPC to `space_room.proto` and an impl method here without
//! touching `main.rs` or the service registration again. Until then it exposes
//! nothing and persists nothing.

use mp_contracts::model_plane::v1::space_room_read_service_server::SpaceRoomReadService;

/// Handler state for the Space room read RPCs. Holds nothing yet; the consuming
/// slices add the pool and any flags they need.
#[derive(Clone, Debug, Default)]
pub(crate) struct SpaceRoomReadGrpc;

impl SpaceRoomReadService for SpaceRoomReadGrpc {}
