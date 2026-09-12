//! End-to-end handshake/login tests: `HostSession` driven by
//! `va-testkit`'s `FakeController`, exactly mirroring how
//! `@verevon/remote-core`'s own tests drive its client-side state machine
//! against `FakeRustDeskPeer.ts`. This is the real proof the state machine
//! works — the unit tests in `messages.rs` only prove individual message
//! shapes round-trip, not that the full sequence actually completes.

use va_crypto::IdentityKeyPair;
use va_protocol::{Credentials, DisplayInfo, Event, HostIdentity, HostInfo, HostSession};
use va_testkit::{FakeController, LoginOutcome};

fn make_host(device_id: &str, password: &str) -> (HostSession, Vec<u8>, [u8; 32]) {
    let identity = IdentityKeyPair::generate();
    let public_key = identity.public_key();
    let (session, signed_id_envelope) = HostSession::start(
        HostIdentity {
            keypair: identity,
            device_id: device_id.to_string(),
        },
        HostInfo {
            hostname: "TEST-DESKTOP".to_string(),
            platform: "Windows".to_string(),
            displays: vec![DisplayInfo {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                name: "Display1".to_string(),
                online: true,
                cursor_embedded: false,
                original_resolution: None,
                scale: 1.0,
            }],
            version: "0.1.0-test".to_string(),
        },
        Credentials {
            permanent_password: None,
            temporary_password: Some(password.to_string()),
        },
    );
    (session, signed_id_envelope, public_key)
}

#[test]
fn full_handshake_and_login_succeeds_with_the_right_password() {
    let (mut host, signed_id_envelope, host_public_key) = make_host("141589119", "correct-horse");
    let mut controller = FakeController::new(host_public_key);

    // 1. Controller receives the host's plaintext SignedId, verifies it,
    //    and replies with PublicKey (still plaintext).
    let public_key_envelope = controller.on_signed_id(&signed_id_envelope).unwrap();

    // 2. Host unseals the session key from PublicKey and sends Hash,
    //    encrypted from this point on.
    let hash_ciphertext = host.on_plaintext_message(&public_key_envelope).unwrap();

    // 3. Controller decrypts Hash, builds LoginRequest with the correct
    //    password, sends it encrypted.
    let hash_plaintext = controller.decrypt(&hash_ciphertext).unwrap();
    let (field_number, hash_payload) = va_protocol::read_envelope_field(&hash_plaintext).unwrap().unwrap();
    assert_eq!(field_number, va_protocol::message_field::HASH);
    let hash = {
        let mut r = va_wire::ProtoReader::new(hash_payload);
        let mut salt = String::new();
        let mut challenge = String::new();
        while !r.eof() {
            let tag = r.read_tag().unwrap();
            match tag.field_number {
                1 => salt = r.read_string().unwrap(),
                2 => challenge = r.read_string().unwrap(),
                _ => r.skip(tag.wire_type).unwrap(),
            }
        }
        (salt, challenge)
    };
    let login_request_ciphertext = controller
        .build_login_request("141589119", "correct-horse", &hash.0, &hash.1)
        .unwrap();

    // 4. Host decrypts LoginRequest, validates the password, and responds
    //    with LoginResponse::Success — reaching Connected.
    let event = host.on_encrypted_message(&login_request_ciphertext).unwrap();
    let response_ciphertext = match event {
        Event::LoggedIn { response, .. } => response,
        Event::LoginFailed { reason, .. } => panic!("expected LoggedIn, got LoginFailed({reason})"),
        _ => panic!("expected LoggedIn"),
    };
    assert!(host.is_connected());

    // 5. Controller decrypts and parses LoginResponse, confirming success
    //    and the expected PeerInfo fields.
    match controller.parse_login_response(&response_ciphertext).unwrap() {
        LoginOutcome::Success { hostname, platform } => {
            assert_eq!(hostname, "TEST-DESKTOP");
            assert_eq!(platform, "Windows");
        }
        LoginOutcome::Failed { reason } => panic!("expected Success, got Failed({reason})"),
    }
}

#[test]
fn login_fails_with_the_wrong_password_but_the_connection_stays_open_for_a_retry() {
    let (mut host, signed_id_envelope, host_public_key) = make_host("141589119", "correct-horse");
    let mut controller = FakeController::new(host_public_key);

    let public_key_envelope = controller.on_signed_id(&signed_id_envelope).unwrap();
    let hash_ciphertext = host.on_plaintext_message(&public_key_envelope).unwrap();
    let hash_plaintext = controller.decrypt(&hash_ciphertext).unwrap();
    let (_, hash_payload) = va_protocol::read_envelope_field(&hash_plaintext).unwrap().unwrap();
    let (salt, challenge) = {
        let mut r = va_wire::ProtoReader::new(hash_payload);
        let mut salt = String::new();
        let mut challenge = String::new();
        while !r.eof() {
            let tag = r.read_tag().unwrap();
            match tag.field_number {
                1 => salt = r.read_string().unwrap(),
                2 => challenge = r.read_string().unwrap(),
                _ => r.skip(tag.wire_type).unwrap(),
            }
        }
        (salt, challenge)
    };

    let wrong_login_ciphertext = controller
        .build_login_request("141589119", "totally-wrong-password", &salt, &challenge)
        .unwrap();
    let event = host.on_encrypted_message(&wrong_login_ciphertext).unwrap();
    let response_ciphertext = match event {
        Event::LoginFailed { reason, response } => {
            assert_eq!(reason, "Wrong Password");
            response
        }
        Event::LoggedIn { .. } => panic!("expected LoginFailed, got LoggedIn"),
        _ => panic!("expected LoginFailed"),
    };
    assert!(!host.is_connected());

    match controller.parse_login_response(&response_ciphertext).unwrap() {
        LoginOutcome::Failed { reason } => assert_eq!(reason, "Wrong Password"),
        LoginOutcome::Success { .. } => panic!("expected Failed"),
    }

    // Retry on the SAME connection with the correct password now succeeds —
    // proves the host doesn't tear down the session on one bad attempt.
    let retry_ciphertext = controller
        .build_login_request("141589119", "correct-horse", &salt, &challenge)
        .unwrap();
    let event = host.on_encrypted_message(&retry_ciphertext).unwrap();
    assert!(matches!(event, Event::LoggedIn { .. }));
    assert!(host.is_connected());
}

#[test]
fn signature_from_a_different_identity_is_rejected() {
    let (host, signed_id_envelope, _real_public_key) = make_host("141589119", "correct-horse");
    drop(host);

    // The controller is told the WRONG public key (as if it had been given
    // a different device's vouch) — verification must fail closed.
    let wrong_identity = IdentityKeyPair::generate();
    let mut controller = FakeController::new(wrong_identity.public_key());
    let result = controller.on_signed_id(&signed_id_envelope);
    assert!(result.is_err(), "expected signature verification to fail against the wrong public key");
}

#[test]
fn test_delay_probe_from_client_is_echoed_verbatim_once_connected() {
    let (mut host, signed_id_envelope, host_public_key) = make_host("141589119", "correct-horse");
    let mut controller = FakeController::new(host_public_key);

    let public_key_envelope = controller.on_signed_id(&signed_id_envelope).unwrap();
    let hash_ciphertext = host.on_plaintext_message(&public_key_envelope).unwrap();
    let hash_plaintext = controller.decrypt(&hash_ciphertext).unwrap();
    let (_, hash_payload) = va_protocol::read_envelope_field(&hash_plaintext).unwrap().unwrap();
    let (salt, challenge) = {
        let mut r = va_wire::ProtoReader::new(hash_payload);
        let mut salt = String::new();
        let mut challenge = String::new();
        while !r.eof() {
            let tag = r.read_tag().unwrap();
            match tag.field_number {
                1 => salt = r.read_string().unwrap(),
                2 => challenge = r.read_string().unwrap(),
                _ => r.skip(tag.wire_type).unwrap(),
            }
        }
        (salt, challenge)
    };
    let login_ciphertext = controller
        .build_login_request("141589119", "correct-horse", &salt, &challenge)
        .unwrap();
    let event = host.on_encrypted_message(&login_ciphertext).unwrap();
    let Event::LoggedIn { response: response_ciphertext, .. } = event else {
        panic!("expected LoggedIn");
    };
    // The controller MUST decrypt this (even though the test doesn't care
    // about its contents here) to keep both sides' independent nonce
    // counters in lockstep — skipping it would desync them, exactly the
    // failure this cipher scheme is supposed to catch.
    controller.parse_login_response(&response_ciphertext).unwrap();

    // Controller sends its own RTT probe; host must echo it back verbatim.
    let probe = va_protocol::TestDelay {
        time: 999,
        from_client: true,
        last_delay: 0,
        target_bitrate: 0,
    };
    let probe_envelope = va_protocol::wrap_message(
        va_protocol::message_field::TEST_DELAY,
        &va_protocol::encode_test_delay(probe),
    );
    let probe_ciphertext = controller.encrypt(&probe_envelope).unwrap();

    let event = host.on_encrypted_message(&probe_ciphertext).unwrap();
    let Event::SendEncrypted(echoed_ciphertext) = event else {
        panic!("expected SendEncrypted (the echo)");
    };
    let echoed_plaintext = controller.decrypt(&echoed_ciphertext).unwrap();
    let (field_number, payload) = va_protocol::read_envelope_field(&echoed_plaintext).unwrap().unwrap();
    assert_eq!(field_number, va_protocol::message_field::TEST_DELAY);
    let echoed = va_protocol::decode_test_delay(payload).unwrap();
    assert_eq!(echoed, probe, "the exact same TestDelay must come back unchanged");
}

/// Drives a full handshake+login (always with `make_host`'s temporary-only
/// password, so `requires_consent` is always `true`) and returns the
/// connected `(host, controller)` pair, ready for session-message tests.
/// Does NOT grant input permission -- callers that need it granted call
/// `host.grant_input_permission()` themselves, since whether/when that
/// happens is exactly what most of these tests are about.
fn make_connected_pair() -> (HostSession, FakeController) {
    let (mut host, signed_id_envelope, host_public_key) = make_host("141589119", "correct-horse");
    let mut controller = FakeController::new(host_public_key);

    let public_key_envelope = controller.on_signed_id(&signed_id_envelope).unwrap();
    let hash_ciphertext = host.on_plaintext_message(&public_key_envelope).unwrap();
    let hash_plaintext = controller.decrypt(&hash_ciphertext).unwrap();
    let (_, hash_payload) = va_protocol::read_envelope_field(&hash_plaintext).unwrap().unwrap();
    let (salt, challenge) = {
        let mut r = va_wire::ProtoReader::new(hash_payload);
        let mut salt = String::new();
        let mut challenge = String::new();
        while !r.eof() {
            let tag = r.read_tag().unwrap();
            match tag.field_number {
                1 => salt = r.read_string().unwrap(),
                2 => challenge = r.read_string().unwrap(),
                _ => r.skip(tag.wire_type).unwrap(),
            }
        }
        (salt, challenge)
    };
    let login_ciphertext = controller
        .build_login_request("141589119", "correct-horse", &salt, &challenge)
        .unwrap();
    let Event::LoggedIn { response: response_ciphertext, requires_consent } =
        host.on_encrypted_message(&login_ciphertext).unwrap()
    else {
        panic!("expected LoggedIn");
    };
    assert!(requires_consent, "make_host only ever sets a temporary password");
    controller.parse_login_response(&response_ciphertext).unwrap();
    (host, controller)
}

fn send_mouse_down_event(host: &mut HostSession, controller: &mut FakeController) -> Event {
    // A left-button-down MouseEvent at (100, 200) -- mask = (0x01 << 3) | 1.
    let mut mouse_writer = va_wire::ProtoWriter::new();
    mouse_writer.int32(1, 9).sint32(2, 100).sint32(3, 200);
    let mouse_envelope = va_protocol::wrap_message(va_protocol::message_field::MOUSE_EVENT, &mouse_writer.finish());
    let mouse_ciphertext = controller.encrypt(&mouse_envelope).unwrap();
    host.on_encrypted_message(&mouse_ciphertext).unwrap()
}

fn send_return_key_down_event(host: &mut HostSession, controller: &mut FakeController) -> Event {
    // A Return key-down (KeyEvent.mode omitted -- proto3 default Legacy).
    let mut key_writer = va_wire::ProtoWriter::new();
    key_writer.bool(1, true).int32(3, 27);
    let key_envelope = va_protocol::wrap_message(va_protocol::message_field::KEY_EVENT, &key_writer.finish());
    let key_ciphertext = controller.encrypt(&key_envelope).unwrap();
    host.on_encrypted_message(&key_ciphertext).unwrap()
}

/// Decodes a `Misc.permission_info` envelope (as returned by
/// `grant_input_permission`/`deny_input_permission`, decrypted by the
/// controller) into `(permission, enabled)` -- mirrors `remote-core`'s own
/// `decodeMisc` on the browser side, hand-rolled here since `va-testkit`
/// has no dedicated decoder for it (this crate never needed to DECODE its
/// own `PermissionInfo` output before now; only encode it).
fn decode_permission_info_envelope(plaintext: &[u8]) -> (i32, bool) {
    let (field_number, misc_payload) = va_protocol::read_envelope_field(plaintext).unwrap().unwrap();
    assert_eq!(field_number, va_protocol::message_field::MISC);
    let mut r = va_wire::ProtoReader::new(misc_payload);
    let tag = r.read_tag().unwrap();
    assert_eq!(tag.field_number, 6, "Misc.permission_info");
    let info_payload = r.read_length_delimited().unwrap();
    let mut ir = va_wire::ProtoReader::new(info_payload);
    let mut permission = 0;
    let mut enabled = false;
    while !ir.eof() {
        let tag = ir.read_tag().unwrap();
        match tag.field_number {
            1 => permission = ir.read_int32().unwrap(),
            2 => enabled = ir.read_bool().unwrap(),
            _ => ir.skip(tag.wire_type).unwrap(),
        }
    }
    (permission, enabled)
}

#[test]
fn mouse_and_key_events_are_denied_until_input_permission_is_granted() {
    let (mut host, mut controller) = make_connected_pair();

    match send_mouse_down_event(&mut host, &mut controller) {
        Event::InputDenied => {}
        _ => panic!("expected InputDenied before any grant"),
    }
    match send_return_key_down_event(&mut host, &mut controller) {
        Event::InputDenied => {}
        _ => panic!("expected InputDenied before any grant"),
    }

    let grant_ciphertext = host.grant_input_permission().unwrap();
    controller.decrypt(&grant_ciphertext).unwrap(); // keep nonce counters in lockstep

    match send_mouse_down_event(&mut host, &mut controller) {
        Event::MouseInput(mouse) => {
            assert_eq!(mouse.kind, va_protocol::MouseEventKind::Down);
            assert!(mouse.button_flags.contains(va_protocol::MouseButtonFlags::LEFT));
            assert_eq!(mouse.x, 100);
            assert_eq!(mouse.y, 200);
        }
        _ => panic!("expected MouseInput once granted"),
    }
    match send_return_key_down_event(&mut host, &mut controller) {
        Event::KeyInput(key) => {
            assert!(key.down);
            assert_eq!(key.identification, va_protocol::KeyIdentification::ControlKey(27));
            assert_eq!(key.mode, va_protocol::KeyboardMode::Legacy);
        }
        _ => panic!("expected KeyInput once granted"),
    }
}

#[test]
fn grant_input_permission_sends_a_real_decodable_permission_info() {
    let (mut host, mut controller) = make_connected_pair();
    let grant_ciphertext = host.grant_input_permission().unwrap();
    let plaintext = controller.decrypt(&grant_ciphertext).unwrap();
    let (permission, enabled) = decode_permission_info_envelope(&plaintext);
    assert_eq!(permission, 0, "Permission::Keyboard's wire value");
    assert!(enabled);
}

#[test]
fn deny_input_permission_sends_permission_info_and_input_stays_denied() {
    let (mut host, mut controller) = make_connected_pair();
    let deny_ciphertext = host.deny_input_permission().unwrap();
    let plaintext = controller.decrypt(&deny_ciphertext).unwrap();
    let (permission, enabled) = decode_permission_info_envelope(&plaintext);
    assert_eq!(permission, 0, "Permission::Keyboard's wire value");
    assert!(!enabled);

    match send_mouse_down_event(&mut host, &mut controller) {
        Event::InputDenied => {}
        _ => panic!("expected InputDenied after an explicit deny"),
    }
}

#[test]
fn a_permanent_password_login_does_not_require_consent() {
    let identity = IdentityKeyPair::generate();
    let public_key = identity.public_key();
    let (mut host, signed_id_envelope) = HostSession::start(
        HostIdentity { keypair: identity, device_id: "141589119".to_string() },
        HostInfo {
            hostname: "TEST-DESKTOP".to_string(),
            platform: "Windows".to_string(),
            displays: vec![],
            version: "0.1.0-test".to_string(),
        },
        Credentials {
            permanent_password: Some("pre-authorized-secret".to_string()),
            temporary_password: None,
        },
    );
    let mut controller = FakeController::new(public_key);

    let public_key_envelope = controller.on_signed_id(&signed_id_envelope).unwrap();
    let hash_ciphertext = host.on_plaintext_message(&public_key_envelope).unwrap();
    let hash_plaintext = controller.decrypt(&hash_ciphertext).unwrap();
    let (_, hash_payload) = va_protocol::read_envelope_field(&hash_plaintext).unwrap().unwrap();
    let (salt, challenge) = {
        let mut r = va_wire::ProtoReader::new(hash_payload);
        let mut salt = String::new();
        let mut challenge = String::new();
        while !r.eof() {
            let tag = r.read_tag().unwrap();
            match tag.field_number {
                1 => salt = r.read_string().unwrap(),
                2 => challenge = r.read_string().unwrap(),
                _ => r.skip(tag.wire_type).unwrap(),
            }
        }
        (salt, challenge)
    };
    let login_ciphertext = controller
        .build_login_request("141589119", "pre-authorized-secret", &salt, &challenge)
        .unwrap();
    let Event::LoggedIn { requires_consent, .. } = host.on_encrypted_message(&login_ciphertext).unwrap() else {
        panic!("expected LoggedIn");
    };
    assert!(!requires_consent, "a permanent password models pre-authorized, unattended access");
}
