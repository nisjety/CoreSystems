"""Cross-language parity tests for the Model Plane Python envelope leg."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from mp_events import Envelope, derive_idempotency_hash

FIXTURE = (
    Path(__file__).resolve().parents[3]
    / "rust"
    / "crates"
    / "mp-events"
    / "tests"
    / "fixtures"
    / "envelope_valid.json"
)

GOLDEN_IDEMPOTENCY_HEX = (
    "fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637"
)


def test_fixture_file_exists() -> None:
    assert FIXTURE.is_file(), f"missing shared fixture: {FIXTURE}"


def test_envelope_decodes_canonical_fixture() -> None:
    raw = FIXTURE.read_bytes()
    env = Envelope.decode(raw)

    assert env.event_id == "01HXYZ01234567890ABCDEFGHI"
    assert env.event_type == "RUN_STARTED"
    assert env.schema_version == 1
    assert env.producer == "model-gateway"
    assert env.correlation_id == "corr-001"
    assert env.causation_id == "cause-001"
    assert env.idempotency_key == "req-001"
    assert env.org_id == "01HORG01234567890ABCDEFGHI"
    assert env.user_id == "01HUSR01234567890ABCDEFGHI"
    assert env.resource_ref == "run/01HRUN01234567890ABCDEFGHI"
    assert env.payload == {"goal": "test run", "agent_id": "general-v1"}


def test_envelope_roundtrip_preserves_semantic_equality() -> None:
    raw = FIXTURE.read_bytes()
    env = Envelope.decode(raw)
    encoded = env.encode()

    # Roundtrip must be semantically equal even if byte-exact ordering differs
    # from the source fixture.
    assert json.loads(encoded) == json.loads(raw)
    assert Envelope.decode(encoded) == env


def test_derive_idempotency_hash_matches_go_and_rust_golden() -> None:
    digest = derive_idempotency_hash(
        "model-gateway",
        "INGRESS_ACCEPTED",
        "thread/abc",
        "req-1",
    )
    assert digest == GOLDEN_IDEMPOTENCY_HEX


@pytest.mark.parametrize(
    ("producer", "event_type", "resource_ref", "idempotency_key"),
    [
        ("a", "b", "c", "d"),
        ("model-gateway", "RUN_STARTED", "run/xyz", ""),
    ],
)
def test_derive_idempotency_hash_is_deterministic(
    producer: str,
    event_type: str,
    resource_ref: str,
    idempotency_key: str,
) -> None:
    first = derive_idempotency_hash(producer, event_type, resource_ref, idempotency_key)
    second = derive_idempotency_hash(producer, event_type, resource_ref, idempotency_key)
    assert first == second
    assert len(first) == 64
