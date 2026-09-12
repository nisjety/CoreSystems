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
        Event::LoggedIn { response } => response,
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
    let Event::LoggedIn { response: response_ciphertext } = event else {
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

#[test]
fn mouse_and_key_events_decode_into_input_events_once_connected() {
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
    let Event::LoggedIn { response: response_ciphertext } = host.on_encrypted_message(&login_ciphertext).unwrap()
    else {
        panic!("expected LoggedIn");
    };
    controller.parse_login_response(&response_ciphertext).unwrap();

    // A left-button-down MouseEvent at (100, 200) -- mask = (0x01 << 3) | 1.
    let mut mouse_writer = va_wire::ProtoWriter::new();
    mouse_writer.int32(1, 9).sint32(2, 100).sint32(3, 200);
    let mouse_envelope = va_protocol::wrap_message(va_protocol::message_field::MOUSE_EVENT, &mouse_writer.finish());
    let mouse_ciphertext = controller.encrypt(&mouse_envelope).unwrap();
    match host.on_encrypted_message(&mouse_ciphertext).unwrap() {
        Event::MouseInput(mouse) => {
            assert_eq!(mouse.kind, va_protocol::MouseEventKind::Down);
            assert!(mouse.button_flags.contains(va_protocol::MouseButtonFlags::LEFT));
            assert_eq!(mouse.x, 100);
            assert_eq!(mouse.y, 200);
        }
        _ => panic!("expected MouseInput"),
    }

    // A Return key-down (KeyEvent.mode omitted -- proto3 default Legacy).
    let mut key_writer = va_wire::ProtoWriter::new();
    key_writer.bool(1, true).int32(3, 27);
    let key_envelope = va_protocol::wrap_message(va_protocol::message_field::KEY_EVENT, &key_writer.finish());
    let key_ciphertext = controller.encrypt(&key_envelope).unwrap();
    match host.on_encrypted_message(&key_ciphertext).unwrap() {
        Event::KeyInput(key) => {
            assert!(key.down);
            assert_eq!(key.identification, va_protocol::KeyIdentification::ControlKey(27));
            assert_eq!(key.mode, va_protocol::KeyboardMode::Legacy);
        }
        _ => panic!("expected KeyInput"),
    }
}
