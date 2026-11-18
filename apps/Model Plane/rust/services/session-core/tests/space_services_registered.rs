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

/// WS12-S4: `MessageRevisionService` edits and deletes a room message, so it is reachable only
/// through the verifying interceptor, never through a bare `::new`.
#[test]
fn the_message_revision_service_is_registered_behind_the_interceptor() {
    let source = grpc_source();
    let server = "MessageRevisionServiceServer";
    let registration = format!("{server}::with_interceptor(");
    assert!(
        source.contains(&format!(".add_service({registration}")),
        "{server} must be added to the server with the verifying interceptor"
    );
    assert!(
        !source.contains(&format!("{server}::new(")),
        "{server} must never be registered without the interceptor"
    );
    // The interceptor argument is the shared `auth`, as for every other service on the port.
    let start = source.find(&registration).expect("the registration");
    let call = &source[start..];
    let end = call.find("))").expect("the registration closes");
    assert!(
        call[..end].contains("auth.clone()"),
        "{server} must be given the same verifier as the other services"
    );
}
