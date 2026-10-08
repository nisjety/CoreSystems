package insights

import (
	"context"
	"testing"
	"time"
)

func TestOwnerOutcomesVerifyZerosWithoutCertifyingOtherSources(t *testing.T) {
	now := time.Now().UTC()
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})), WithNow(func() time.Time { return now }))
	from, to := now.Add(-time.Hour), now
	query := OverviewQuery{OrgID: "org-a", ActorUserID: "user-a", From: &from, To: &to}
	snapshot := OutcomeSnapshot{OrgID: "org-a", ActorUserID: "user-a", From: &from, To: &to, CheckedAt: now, Source: "conversation-core", ApprovedReviews: 3, RejectedReviews: 1}
	report, err := service.OverviewWithOutcomes(context.Background(), query, snapshot)
	if err != nil {
		t.Fatal(err)
	}
	if report.Coverage != "unknown" {
		t.Fatal("owner outcome read must not certify unrelated event delivery")
	}
	if len(report.Scorecards) != 3 {
		t.Fatalf("scorecards=%d", len(report.Scorecards))
	}
	for _, card := range report.Scorecards {
		if card.Coverage != "measured" || card.Aggregation != "owner_record_count" || card.CheckedAt == nil {
			t.Fatalf("unverified owner card: %#v", card)
		}
	}
	for _, mutate := range []func(*OutcomeSnapshot){
		func(s *OutcomeSnapshot) { s.OrgID = "org-b" }, func(s *OutcomeSnapshot) { s.ActorUserID = "user-b" },
		func(s *OutcomeSnapshot) { s.From = nil }, func(s *OutcomeSnapshot) { s.Source = "browser" },
		func(s *OutcomeSnapshot) { s.CheckedAt = now.Add(-3 * time.Minute) }, func(s *OutcomeSnapshot) { s.ResolvedTickets = -1 },
	} {
		bad := snapshot
		mutate(&bad)
		if _, err := service.OverviewWithOutcomes(context.Background(), query, bad); err == nil {
			t.Fatal("invalid source proof accepted")
		}
	}
}
