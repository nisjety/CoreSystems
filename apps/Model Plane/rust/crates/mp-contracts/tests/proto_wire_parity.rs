use mp_contracts::model_plane::v1::Event;
use prost::Message;
use prost_types::{Any, Timestamp};

const GOLDEN_EVENT_WIRE_HEX: &str = "0a1a3031485637413451324d37533545394a365238433254315a345810501801220b08f1f8dacf0610c0a9d33a2a0d6d6f64656c2d676174657761793208636f72722d3030313a0963617573652d30303142077265712d3030314a1a3031484f52473031323334353637383930414243444546474849521a30314855535230313233343536373839304142434445464748495a207468726561642f3031485448524541443031323334353637383930414243444562330a2d747970652e676f6f676c65617069732e636f6d2f6d6f64656c5f706c616e652e76312e52756e5374617274656412027b7d";

fn canonical_event() -> Event {
    Event {
        event_id: "01HV7A4Q2M7S5E9J6R8C2T1Z4X".to_string(),
        event_type: 80,
        schema_version: 1,
        ts: Some(Timestamp {
            seconds: 1_777_777_777,
            nanos: 123_000_000,
        }),
        producer: "model-gateway".to_string(),
        correlation_id: "corr-001".to_string(),
        causation_id: "cause-001".to_string(),
        idempotency_key: "req-001".to_string(),
        org_id: "01HORG01234567890ABCDEFGHI".to_string(),
        user_id: "01HUSR01234567890ABCDEFGHI".to_string(),
        resource_ref: "thread/01HTHREAD01234567890ABCDE".to_string(),
        payload: Some(Any {
            type_url: "type.googleapis.com/model_plane.v1.RunStarted".to_string(),
            value: b"{}".to_vec(),
        }),
        zdr: false,
    }
}

fn hex_encode_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[test]
fn event_encode_matches_golden_hex_red() {
    let encoded = canonical_event().encode_to_vec();
    let got_hex = hex_encode_lower(&encoded);

    assert_eq!(got_hex, GOLDEN_EVENT_WIRE_HEX);
}

#[test]
fn event_roundtrip_decodes_back_to_same_values() {
    let original = canonical_event();
    let encoded = original.encode_to_vec();
    let decoded = Event::decode(encoded.as_slice()).expect("decode encoded event");

    assert_eq!(decoded.event_id, original.event_id);
    assert_eq!(decoded.event_type, original.event_type);
    assert_eq!(decoded.schema_version, original.schema_version);
    assert_eq!(decoded.producer, original.producer);
    assert_eq!(decoded.correlation_id, original.correlation_id);
    assert_eq!(decoded.idempotency_key, original.idempotency_key);
    assert_eq!(decoded.resource_ref, original.resource_ref);
}

#[test]
fn event_decode_rejects_truncated_payload() {
    let bad_wire = [0x0A, 0x02, 0x41];
    let err = Event::decode(bad_wire.as_slice()).expect_err("expected decode failure");
    assert!(
        err.to_string().contains("buffer") || err.to_string().contains("eof"),
        "unexpected decode error: {err}"
    );
}
