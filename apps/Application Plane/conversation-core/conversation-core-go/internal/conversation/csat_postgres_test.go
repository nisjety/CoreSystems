package conversation

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/database"
)

// TestPGRepositoryTicketCSATOutcomeOrgScopingRejectsCrossOrgAccess is a real,
// Postgres-backed integration test against the actual PGRepository
// implementation. Every other test that touches CSAT behaviour in this
// package exercises service.go's validation logic or a fake repository;
// none of them can prove that ticket_csat_outcomes' SQL-level
// `WHERE org_id = $1 AND ticket_id = $2` scoping (see GetTicketCSATOutcome)
// and the org-scoped `conversation_tickets` join (see the CTE in
// UpsertTicketCSATOutcome) genuinely reject a cross-org access attempt
// instead of just happening to look right.
//
// This test seeds a ticket and a CSAT outcome that belong to org_2 directly
// via SQL, then calls the real PGRepository.GetTicketCSATOutcome and
// PGRepository.UpsertTicketCSATOutcome as org_1 and asserts:
//  1. both calls return ErrNotFound rather than exposing or mutating org_2's
//     row, and
//  2. a read back with the correct org confirms org_2's row was left
//     untouched, and
//  3. the same calls still succeed when made with the correct, owning org_2
//     -- proving the cross-org rejection above is not vacuous.
//
// It is gated behind TEST_DATABASE_URL and skipped when unset, matching the
// convention already used by this module's other live-Postgres suite
// (internal/http/agent_ticket_grants_live_test.go). The caller must point
// that env var at a disposable, throwaway Postgres database -- this test
// applies conversation-core-go's real migrations (internal/database/migrations)
// and must never be pointed at a live or shared conversation-core database.
//
// Fixture rows use fixed ids (rather than randomly generated ones) so that
// the SQL seeded in this file is easy to read and cross-reference against
// the assertions below. Because the ids are fixed, this test resets its own
// fixtures both before and after running so it stays safe to re-run
// repeatedly against the same disposable database.
const (
	csatOrgScopingInboxID        = "csat_orgscope_inbox"
	csatOrgScopingConversationID = "csat_orgscope_conversation"
	csatOrgScopingTicketID       = "csat_orgscope_ticket"
	csatOrgScopingTicketKey      = "CSAT-ORGSCOPE-1"
	csatOrgScopingOwningOrg      = "org_2"
	csatOrgScopingAttackerOrg    = "org_1"
)

func TestPGRepositoryTicketCSATOutcomeOrgScopingRejectsCrossOrgAccess(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()

	if err := database.RunMigrations(ctx, db); err != nil {
		t.Fatalf("apply conversation-core migrations: %v", err)
	}

	repo := NewRepository(db.Pool)

	// conversations.inbox_id is ON DELETE RESTRICT, so the conversation must
	// be removed before the inbox. ticket_csat_outcomes.ticket_id and
	// .conversation_id both cascade from conversation_tickets/conversations,
	// so deleting the conversation alone clears the ticket and the outcome
	// row too. This intentionally runs with the test's own ctx/db, both of
	// which are still live here -- it is NOT wired through t.Cleanup, because
	// t.Cleanup callbacks run only after this function's own `defer cancel()`
	// and `defer db.Close()` have already fired, which would hand the
	// cleanup query an already-canceled context and an already-closed pool.
	resetOrgScopingFixtures := func() {
		if _, err := db.Pool.Exec(ctx, `DELETE FROM conversations WHERE id = $1`, csatOrgScopingConversationID); err != nil {
			t.Fatalf("reset org-scoping conversation fixture: %v", err)
		}
		if _, err := db.Pool.Exec(ctx, `DELETE FROM conversation_inboxes WHERE id = $1`, csatOrgScopingInboxID); err != nil {
			t.Fatalf("reset org-scoping inbox fixture: %v", err)
		}
	}
	resetOrgScopingFixtures()

	if _, err := db.Pool.Exec(ctx, `
INSERT INTO conversation_inboxes (id, org_id, name, channel)
VALUES ($1, $2, 'CSAT org-scoping fixture inbox', 'email')`,
		csatOrgScopingInboxID, csatOrgScopingOwningOrg); err != nil {
		t.Fatalf("seed org_2 inbox: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO conversations (id, org_id, inbox_id, title, status, priority)
VALUES ($1, $2, $3, 'CSAT org-scoping fixture conversation', 'open', 'normal')`,
		csatOrgScopingConversationID, csatOrgScopingOwningOrg, csatOrgScopingInboxID); err != nil {
		t.Fatalf("seed org_2 conversation: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO conversation_tickets (id, org_id, conversation_id, ticket_key, status)
VALUES ($1, $2, $3, $4, 'resolved')`,
		csatOrgScopingTicketID, csatOrgScopingOwningOrg, csatOrgScopingConversationID, csatOrgScopingTicketKey); err != nil {
		t.Fatalf("seed org_2 ticket: %v", err)
	}
	// Seed the CSAT outcome directly via SQL (rather than through the
	// repository) so this test exercises PGRepository's read/write org
	// scoping against a row it did not itself create, exactly as an
	// attacker would encounter a pre-existing row belonging to another org.
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO ticket_csat_outcomes (org_id, ticket_id, conversation_id, score, recorded_by)
VALUES ($1, $2, $3, 5, 'seed-agent')`,
		csatOrgScopingOwningOrg, csatOrgScopingTicketID, csatOrgScopingConversationID); err != nil {
		t.Fatalf("seed org_2 CSAT outcome: %v", err)
	}

	// Cross-org read: org_1 must not be able to see org_2's outcome.
	outcome, err := repo.GetTicketCSATOutcome(ctx, csatOrgScopingAttackerOrg, csatOrgScopingTicketID)
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-org GetTicketCSATOutcome error = %v, want ErrNotFound", err)
	}
	if outcome != nil {
		t.Fatalf("cross-org GetTicketCSATOutcome returned a non-nil outcome: %+v", outcome)
	}

	// Cross-org write: org_1 must not be able to upsert (create or overwrite)
	// org_2's outcome for the same ticket_id.
	outcome, err = repo.UpsertTicketCSATOutcome(ctx, TicketCSATOutcomeInput{
		OrgID:      csatOrgScopingAttackerOrg,
		TicketID:   csatOrgScopingTicketID,
		Score:      1,
		RecordedBy: "attacker-actor",
	})
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-org UpsertTicketCSATOutcome error = %v, want ErrNotFound", err)
	}
	if outcome != nil {
		t.Fatalf("cross-org UpsertTicketCSATOutcome returned a non-nil outcome: %+v", outcome)
	}

	// Assert org_2's row was NOT mutated by the rejected cross-org attempts.
	var score int
	var recordedBy string
	if err := db.Pool.QueryRow(ctx, `
SELECT score, recorded_by FROM ticket_csat_outcomes WHERE org_id = $1 AND ticket_id = $2`,
		csatOrgScopingOwningOrg, csatOrgScopingTicketID).Scan(&score, &recordedBy); err != nil {
		t.Fatalf("read org_2 outcome after cross-org attempts: %v", err)
	}
	if score != 5 || recordedBy != "seed-agent" {
		t.Fatalf("org_2 outcome mutated by cross-org attempts: score=%d recorded_by=%q", score, recordedBy)
	}

	// Positive case: the legitimate owning org can still read and upsert its
	// own outcome -- proving the cross-org rejections above are not vacuous.
	outcome, err = repo.GetTicketCSATOutcome(ctx, csatOrgScopingOwningOrg, csatOrgScopingTicketID)
	if err != nil {
		t.Fatalf("same-org GetTicketCSATOutcome failed: %v", err)
	}
	if outcome == nil || outcome.Score != 5 || outcome.RecordedBy != "seed-agent" {
		t.Fatalf("same-org GetTicketCSATOutcome did not return the seeded outcome: %+v", outcome)
	}

	outcome, err = repo.UpsertTicketCSATOutcome(ctx, TicketCSATOutcomeInput{
		OrgID:      csatOrgScopingOwningOrg,
		TicketID:   csatOrgScopingTicketID,
		Score:      4,
		RecordedBy: "owning-actor",
	})
	if err != nil {
		t.Fatalf("same-org UpsertTicketCSATOutcome failed: %v", err)
	}
	if outcome == nil || outcome.Score != 4 || outcome.RecordedBy != "owning-actor" {
		t.Fatalf("same-org UpsertTicketCSATOutcome did not apply: %+v", outcome)
	}

	resetOrgScopingFixtures()
}
