//! WS07-S6: `ThreadSummary.memory_mode` (tag 31) round trips on the wire, and an empty value adds no bytes,
//! so a listing that carries no mode encodes exactly as it did before the field existed.

use mp_contracts::model_plane::v1::ThreadSummary;
use prost::Message;

#[test]
fn memory_mode_round_trips_on_tag_31() {
    let original = ThreadSummary {
        thread_id: "thr-memory-mode".to_owned(),
        memory_mode: "off".to_owned(),
        ..Default::default()
    };
    let encoded = original.encode_to_vec();
    // Field 31, wire type 2 (length-delimited): the key is (31 << 3) | 2 = 250, a two-byte varint
    // 0xfa 0x01, then the length 3 and the three bytes of "off".
    assert!(
        encoded
            .windows(6)
            .any(|window| window == [0xfa, 0x01, 0x03, b'o', b'f', b'f']),
        "{encoded:?}"
    );
    let decoded = ThreadSummary::decode(encoded.as_slice()).expect("decode the encoded summary");
    assert_eq!(decoded.memory_mode, "off");
    assert_eq!(decoded.thread_id, original.thread_id);
}

#[test]
fn an_empty_memory_mode_adds_no_bytes_to_the_wire() {
    let bare = ThreadSummary {
        thread_id: "thr-memory-mode".to_owned(),
        ..Default::default()
    };
    let explicit_empty = ThreadSummary {
        thread_id: "thr-memory-mode".to_owned(),
        memory_mode: String::new(),
        ..Default::default()
    };
    assert_eq!(bare.encode_to_vec(), explicit_empty.encode_to_vec());
    let decoded = ThreadSummary::decode(bare.encode_to_vec().as_slice()).expect("decode");
    assert!(decoded.memory_mode.is_empty());
}
