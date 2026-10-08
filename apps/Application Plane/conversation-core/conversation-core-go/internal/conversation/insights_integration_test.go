//go:build integration

package conversation

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/database"
)

// Only run against a disposable owner database. This uses the real migration
// schema and distinguishes current assignees from review decision actors.
func TestOutcomeReportPostgresTenantActorAndWindow(t *testing.T) {
	dsn := os.Getenv("CONVERSATION_CORE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONVERSATION_CORE_TEST_DATABASE_URL is not set")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if err := database.RunMigrations(ctx, db); err != nil {
		t.Fatal(err)
	}
	exec := func(query string, args ...any) {
		t.Helper()
		if _, err := db.Pool.Exec(ctx, query, args...); err != nil {
			t.Fatal(err)
		}
	}
	from := time.Date(2026, 9, 10, 0, 0, 0, 0, time.UTC)
	to := from.Add(24 * time.Hour)
	middle := from.Add(time.Hour)
	for _, org := range []string{"org-a", "org-b"} {
		exec(`INSERT INTO conversation_inboxes (id,org_id,name) VALUES ($1,$2,'Test inbox')`, org+"-inbox", org)
	}
	for i, ticket := range []struct {
		org, status, actor string
		at                 time.Time
	}{
		{"org-a", "resolved", "user-a", from}, {"org-a", "closed", "user-b", middle},
		{"org-a", "open", "user-a", middle}, {"org-a", "resolved", "user-a", from.Add(-time.Second)},
		{"org-a", "resolved", "user-a", to}, {"org-b", "resolved", "user-a", middle},
	} {
		id := fmt.Sprintf("outcome-%d", i)
		exec(`INSERT INTO conversations (id,org_id,inbox_id,title) VALUES ($1,$2,$3,'Test')`, id, ticket.org, ticket.org+"-inbox")
		exec(`INSERT INTO conversation_tickets (id,org_id,conversation_id,ticket_key,status,assignee_user_id,resolved_at) VALUES ($1,$2,$1,$1,$3,$4,$5)`, id, ticket.org, ticket.status, ticket.actor, ticket.at)
	}
	for _, row := range []struct{ org, conversation string }{{"org-a", "outcome-0"}, {"org-b", "outcome-5"}} {
		exec(`INSERT INTO conversation_ai_actions (id,org_id,conversation_id,kind) VALUES ($1,$2,$3,'draft')`, row.org+"-action", row.org, row.conversation)
	}
	for i, review := range []struct {
		org, actor, decision string
		at                   time.Time
	}{
		{"org-a", "user-a", "approved", from}, {"org-a", "user-b", "approved", middle},
		{"org-a", "user-b", "rejected", middle}, {"org-a", "user-a", "rejected", to},
		{"org-b", "user-a", "approved", middle}, {"org-a", "user-a", "approved", from.Add(-time.Second)},
	} {
		exec(`INSERT INTO conversation_ai_reviews (id,org_id,ai_action_id,reviewer_user_id,decision,created_at) VALUES ($1,$2,$3,$4,$5,$6)`, fmt.Sprintf("review-%d", i), review.org, review.org+"-action", review.actor, review.decision, review.at)
	}
	repo := NewRepository(db.Pool)
	for _, want := range []struct {
		org, actor                  string
		tickets, approved, rejected int64
	}{
		{"org-a", "", 2, 2, 1}, {"org-a", "user-a", 1, 1, 0}, {"org-a", "user-b", 1, 1, 1},
		{"org-a", "unknown-user", 0, 0, 0}, {"org-b", "user-a", 1, 1, 0},
	} {
		got, err := repo.ReadOutcomeReport(ctx, want.org, want.actor, &from, &to)
		if err != nil {
			t.Fatal(err)
		}
		if got.ResolvedTickets != want.tickets || got.ApprovedReviews != want.approved || got.RejectedReviews != want.rejected || got.OrgID != want.org || got.ActorUserID != want.actor || got.CheckedAt.IsZero() {
			t.Fatalf("scope=%s/%s got=%+v want=%+v", want.org, want.actor, got, want)
		}
	}
}
