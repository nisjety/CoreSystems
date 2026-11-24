package users

import (
	"os"
	"strings"
	"testing"
)

// D22o (F-t1-016): the erasure saga's local stage removes the erased person
// from every DM roster inside the same transaction as the ledger pseudonym,
// with that pseudonym, and before the mode switch, so hard delete and
// anonymize both do it. The behaviour itself is proven against a database in
// internal/spaces (TestEraseDMParticipantKeepsTheDMAndThePeers).
func TestLocalErasureStageLeavesDMRosters(t *testing.T) {
	raw, err := os.ReadFile("gdpr_erasure_store.go")
	if err != nil {
		t.Fatal(err)
	}
	src := strings.ReplaceAll(string(raw), "\r\n", "\n")
	start := strings.Index(src, "func (r *Repository) CompleteErasureLocalStage(")
	if start < 0 {
		t.Fatal("CompleteErasureLocalStage not found")
	}
	body := src[start:]
	if end := strings.Index(body[1:], "\nfunc "); end > 0 {
		body = body[:end+1]
	}
	call := strings.Index(body, "spaces.EraseDMParticipantTx(ctx, tx, operation.UserID, ledgerPseudonym)")
	pseudonym := strings.Index(body, "ledgerPseudonym :=")
	modeSwitch := strings.Index(body, "switch operation.Mode")
	if call < 0 {
		t.Fatal("the local stage does not erase DM rosters with the ledger pseudonym in its transaction")
	}
	if pseudonym < 0 || pseudonym > call || modeSwitch < 0 || call > modeSwitch {
		t.Fatal("DM roster erasure must follow the pseudonym and precede the mode switch")
	}
}
