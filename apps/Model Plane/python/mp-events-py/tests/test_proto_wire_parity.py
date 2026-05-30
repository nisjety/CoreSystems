from __future__ import annotations

import binascii

import pytest
from google.protobuf.message import DecodeError

from mp_events._gen.model_plane.v1 import events_pb2, orchestration_pb2

GOLDEN_EVENT_WIRE_HEX = "0a1a3031485637413451324d37533545394a365238433254315a345810501801220b08f1f8dacf0610c0a9d33a2a0d6d6f64656c2d676174657761793208636f72722d3030313a0963617573652d30303142077265712d3030314a1a3031484f52473031323334353637383930414243444546474849521a30314855535230313233343536373839304142434445464748495a207468726561642f3031485448524541443031323334353637383930414243444562330a2d747970652e676f6f676c65617069732e636f6d2f6d6f64656c5f706c616e652e76312e52756e5374617274656412027b7d"


def test_event_decode_from_golden_hex_red() -> None:
    wire = binascii.unhexlify(GOLDEN_EVENT_WIRE_HEX)
    event = events_pb2.Event()
    event.ParseFromString(wire)

    assert event.event_id == "01HV7A4Q2M7S5E9J6R8C2T1Z4X"
    assert event.event_type == events_pb2.EVENT_TYPE_INGRESS_ACCEPTED
    assert event.producer == "model-gateway"
    assert event.idempotency_key == "req-001"
    assert event.resource_ref == "thread/01HTHREAD01234567890ABCDE"


def test_event_decode_rejects_truncated_payload() -> None:
    wire = bytes([0x0A, 0x02, 0x41])
    event = events_pb2.Event()
    with pytest.raises(DecodeError):
        event.ParseFromString(wire)


# Cross-language goldens for OrchestrationEvent (7 oneof variants, tags 10-16).
# Hex bytes match the Rust round-trip goldens in
# rust/crates/mp-orchestration/tests/proto_wire_parity.rs and the Go test in
# go/gen/model_plane/v1/proto_wire_parity_orchestration_test.go.
ORCH_GOLDENS = [
    ("plan_transitioned", "0a060880e2cfaa0652130a06706c616e2d31120572756e2d3118012002"),
    ("todo_transitioned", "0a060880e2cfaa065a160a06746f646f2d3112087468726561642d3118012002"),
    (
        "approval_state_changed",
        "0a060880e2cfaa0662250a06617070722d31120572756e2d31180220022a1075736572406578616d706c652e636f6d",
    ),
    ("subagent_attached", "0a060880e2cfaa066a190a0a72756e2d706172656e74120972756e2d6368696c641802"),
    ("subagent_stopped", "0a060880e2cfaa0672160a0972756e2d6368696c641209636f6d706c65746564"),
    ("run_paused_for_approval", "0a060880e2cfaa067a0f0a0572756e2d311206617070722d31"),
    ("run_resumed_after_approval", "0a060880e2cfaa0682010f0a0572756e2d311206617070722d31"),
]


@pytest.mark.parametrize("expected_variant,hex_wire", ORCH_GOLDENS)
def test_orchestration_event_decode(expected_variant: str, hex_wire: str) -> None:
    msg = orchestration_pb2.OrchestrationEvent()
    msg.ParseFromString(binascii.unhexlify(hex_wire))
    assert msg.WhichOneof("event") == expected_variant
    assert msg.HasField("at")