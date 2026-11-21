package grant

// WS21-S4 read level in the in-memory store. The durable store keeps the same
// rules over the read_only column (postgres_read_only_integration_test.go).

import (
	"errors"
	"testing"
	"time"
)

func TestReadOnly_MemoryKeepsTheLevelThroughDeriveAndExtension(t *testing.T) {
	store, _ := clockedRepo()
	root := session(t, store, CreateSessionParams{ReadOnly: true, TTL: 30 * time.Minute})
	if !root.ReadOnly {
		t.Fatal("the session lost the read level at create")
	}
	run, err := store.Derive(ctx, deriveParams(root, "run-1"))
	if err != nil {
		t.Fatalf("derive: %v", err)
	}
	if !run.ReadOnly {
		t.Fatal("a run derived from a read-only session is not read-only")
	}
	ext, err := store.CreateOriginExtension(ctx, extParams(run, ConsentScopeTurn, "a.other.org"))
	if err != nil {
		t.Fatalf("extension under a read-only run: %v", err)
	}
	if !ext.ReadOnly {
		t.Fatal("an extension under a read-only run is not read-only")
	}
	if _, err := store.CreateScopedSensitiveApproval(ctx, approvalOn(run, []string{"example.com"}, time.Minute)); !errors.Is(err, ErrApprovalParentInvalid) {
		t.Fatalf("approval under a read-only run: %v, want ErrApprovalParentInvalid", err)
	}
}

func TestReadOnly_DefaultIsFullAccess(t *testing.T) {
	store, _ := clockedRepo()
	root := session(t, store, CreateSessionParams{TTL: 30 * time.Minute})
	if root.ReadOnly {
		t.Fatal("a session created without the level reads read-only")
	}
	run, err := store.Derive(ctx, deriveParams(root, "run-1"))
	if err != nil {
		t.Fatalf("derive: %v", err)
	}
	if run.ReadOnly {
		t.Fatal("a run derived from a full-access session reads read-only")
	}
}
