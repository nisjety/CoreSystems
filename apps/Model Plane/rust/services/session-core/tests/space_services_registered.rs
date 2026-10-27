//! The Space read services are served behind the verifying interceptor (WS02-S3, WS07-S9).
//!
//! An RPC added to `SpaceRoomReadService` or `SpaceEvents` is reachable only through the
//! interceptor, because both services are registered with `with_interceptor` and never with a
//! bare `::new`. The check is a scan of the registration source, so a future edit that
//! registers either service without the interceptor fails here.

use std::path::Path;

fn grpc_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("grpc.rs");
    std::fs::read_to_string(path).expect("grpc.rs is readable")
}

#[test]
fn both_space_services_are_registered_behind_the_interceptor() {
    let source = grpc_source();
    for server in ["SpaceRoomReadServiceServer", "SpaceEventsServer"] {
        assert!(
            source.contains(&format!("{server}::with_interceptor(")),
            "{server} must be registered with the verifying interceptor"
        );
        assert!(
            !source.contains(&format!("{server}::new(")),
            "{server} must never be registered without the interceptor"
        );
    }
}

#[test]
fn the_health_names_list_both_space_services() {
    let source = grpc_source();
    for name in ["model_plane.v1.SpaceRoomReadService", "model_plane.v1.SpaceEvents"] {
        assert!(source.contains(&format!("\"{name}\"")), "{name} is not in HEALTH_SERVICE_NAMES");
    }
}
