package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"testing"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/api"
	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

// seedZDRAcknowledgementRow gives a model connection an active acknowledgement of its current assessment.
// When revokedAt is not zero it then revokes that row at revokedAt, with the reason a removal leaves
// (connection_removed), so a test can place a revoked row as far in the past as it needs.
func seedZDRAcknowledgementRow(t *testing.T, repo *store.MemoryRepository, connectionID string, revokedAt time.Time) {
	t.Helper()
	ctx := context.Background()
	if _, err := repo.UpsertConnection(ctx, store.Connection{
		ID:             connectionID,
		ProviderKey:    codexsubscription.ProviderKey,
		ConnectorType:  codexsubscription.ConnectorType,
		OrganizationID: "org-1",
		WorkspaceID:    "ws-org-1",
		UserID:         "user-1",
		Status:         "active",
		Capabilities:   []string{codexsubscription.Capability},
	}); err != nil {
		t.Fatalf("seed connection %s: %v", connectionID, err)
	}
	if _, err := api.ReconcileConnectionZDR(ctx, api.ServerConfig{Repo: repo}); err != nil {
		t.Fatalf("assess connections: %v", err)
	}
	record, err := repo.GetConnectionZDR(ctx, connectionID)
	if err != nil {
		t.Fatalf("read the assessment of %s: %v", connectionID, err)
	}
	const text = "the acknowledgement text the member was shown"
	sum := sha256.Sum256([]byte(text))
	if _, _, err := repo.InsertConnectionZDRAcknowledgement(ctx, store.NewConnectionZDRAcknowledgement{
		OrganizationID:  "org-1",
		ConnectionID:    connectionID,
		UserID:          "user-1",
		Digest:          record.Digest,
		TextVersion:     "v1",
		Locale:          "en",
		TextSHA256:      hex.EncodeToString(sum[:]),
		TextSnapshot:    text,
		AcknowledgedVia: "connect",
	}); err != nil {
		t.Fatalf("acknowledge %s: %v", connectionID, err)
	}
	if revokedAt.IsZero() {
		return
	}
	if _, err := repo.RevokeConnectionZDRAcknowledgements(ctx, connectionID, store.ZDRRevokeConnectionRemoved, revokedAt); err != nil {
		t.Fatalf("revoke the acknowledgement of %s: %v", connectionID, err)
	}
}

// ledgerRowCount counts the acknowledgement rows of a connection, revoked ones included.
func ledgerRowCount(t *testing.T, repo *store.MemoryRepository, connectionID string) int {
	t.Helper()
	rows, err := repo.ListConnectionZDRAcknowledgements(context.Background(), connectionID, false)
	if err != nil {
		t.Fatalf("list the acknowledgements of %s: %v", connectionID, err)
	}
	return len(rows)
}

// A row revoked while the switch was on is kept with no time limit while the switch is off: the sweep does
// not start, however old the revoked row is. This pins the behaviour the README and the compose comment
// describe (D-ZDR-16, owner to confirm under OD-LEGAL).
func TestRevokedAcknowledgementIsKeptWhileTheSwitchIsOff(t *testing.T) {
	repo := store.NewMemoryRepository()
	seedZDRAcknowledgementRow(t, repo, "conn-aged", time.Now().UTC().Add(-400*24*time.Hour))
	cfg := config.Config{ZDRAcknowledgementEnabled: false, ZDRAcknowledgementRetentionDays: 30}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	logger := zerolog.Nop()

	startZDRAcknowledgementSweep(ctx, cfg, repo, &logger)

	// A sweep that had started would purge at once, so a short wait is enough to see it.
	time.Sleep(300 * time.Millisecond)
	if got := ledgerRowCount(t, repo, "conn-aged"); got != 1 {
		t.Fatalf("revoked acknowledgement past the window with ZDR_ACKNOWLEDGEMENT_ENABLED off: %d row(s), want 1 kept", got)
	}
}

// With the switch on, the same sweep deletes a revoked row only once it is older than the window. A row
// revoked inside the window and an active row are never deleted by it.
func TestSwitchedOnSweepDeletesRevokedRowsOnlyPastTheWindow(t *testing.T) {
	repo := store.NewMemoryRepository()
	now := time.Now().UTC()
	seedZDRAcknowledgementRow(t, repo, "conn-aged", now.Add(-400*24*time.Hour))
	seedZDRAcknowledgementRow(t, repo, "conn-recent", now.Add(-24*time.Hour))
	seedZDRAcknowledgementRow(t, repo, "conn-active", time.Time{})
	if got := ledgerRowCount(t, repo, "conn-aged"); got != 1 {
		t.Fatalf("setup: %d row(s) for the aged revoked acknowledgement, want 1", got)
	}
	cfg := config.Config{ZDRAcknowledgementEnabled: true, ZDRAcknowledgementRetentionDays: 30}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	logger := zerolog.Nop()

	startZDRAcknowledgementSweep(ctx, cfg, repo, &logger)

	deadline := time.Now().Add(5 * time.Second)
	for ledgerRowCount(t, repo, "conn-aged") != 0 {
		if time.Now().After(deadline) {
			t.Fatal("the sweep did not delete the revoked acknowledgement past the window with the switch on")
		}
		time.Sleep(10 * time.Millisecond)
	}
	if got := ledgerRowCount(t, repo, "conn-recent"); got != 1 {
		t.Fatalf("revoked acknowledgement inside the window: %d row(s), want 1 kept", got)
	}
	if got := ledgerRowCount(t, repo, "conn-active"); got != 1 {
		t.Fatalf("active acknowledgement: %d row(s), want 1 kept", got)
	}
}
