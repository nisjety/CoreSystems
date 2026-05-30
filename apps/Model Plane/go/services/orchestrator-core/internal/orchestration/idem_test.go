package orchestration_test

import (
	"testing"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/orchestration"
)

// TestIdemPrefix_DelegatesToEnvelope ensures the orchestration wrapper composes
// the canonical helper with the orchestrator-core producer identifier. The
// authoritative cross-language golden lives in pkg/envelope.
func TestIdemPrefix_DelegatesToEnvelope(t *testing.T) {
	got := orchestration.IdemPrefix("plan.transitioned", "thread/abc", "req-1")
	want := envelope.DeriveIdempotencyHash(orchestration.Producer, "plan.transitioned", "thread/abc", "req-1")
	if got != want {
		t.Fatalf("IdemPrefix mismatch\n got:  %s\n want: %s", got, want)
	}
}

// TestIdemPrefix_GoldenDigest pins the cross-language golden digest for the
// orchestrator-core producer fixture: blake3("orchestrator-core|plan.transitioned|thread/abc|req-1").
// The same digest must be produced by the Rust (`mp-events::idempotency`) and
// Python implementations to preserve idempotency parity across producers. The
// repo-wide model-gateway fixture (INGRESS_ACCEPTED) is pinned separately in
// pkg/envelope/envelope_test.go.
func TestIdemPrefix_GoldenDigest(t *testing.T) {
	const golden = "6170f7b4e75b6eb6f29b1eae866049136c042d4ec5570f5cd8644bbd60c36b38"
	got := orchestration.IdemPrefix("plan.transitioned", "thread/abc", "req-1")
	if got != golden {
		t.Fatalf("idem prefix golden mismatch:\n got=%s\nwant=%s", got, golden)
	}
}
