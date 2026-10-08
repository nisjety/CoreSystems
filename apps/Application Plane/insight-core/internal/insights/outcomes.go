package insights

import (
	"context"
	"fmt"
	"time"
)

// OutcomeSnapshot is transported by the authenticated BFF after a signed,
// organization-scoped read from Conversation Core. It is not a public ingest
// contract and never persists customer content or rewrites lifecycle events.
type OutcomeSnapshot struct {
	OrgID           string     `json:"org_id"`
	ActorUserID     string     `json:"actor_user_id,omitempty"`
	From            *time.Time `json:"from,omitempty"`
	To              *time.Time `json:"to,omitempty"`
	CheckedAt       time.Time  `json:"checked_at"`
	Source          string     `json:"source"`
	ResolvedTickets int64      `json:"resolved_tickets"`
	ApprovedReviews int64      `json:"approved_reviews"`
	RejectedReviews int64      `json:"rejected_reviews"`
}

func sameBound(a, b *time.Time) bool {
	return a == nil && b == nil || a != nil && b != nil && a.Equal(*b)
}

// OverviewWithOutcomes reconciles only the metrics proven by the owner read.
// Unrelated event series remain explicitly observed; zeros are included only
// for the source queries that actually completed successfully.
func (s *Service) OverviewWithOutcomes(ctx context.Context, query OverviewQuery, snapshot OutcomeSnapshot) (*Overview, error) {
	q, err := normalizeOverviewQuery(query)
	if err != nil {
		return nil, err
	}
	if snapshot.Source != "conversation-core" || snapshot.OrgID != q.OrgID || snapshot.ActorUserID != q.ActorUserID ||
		!sameBound(snapshot.From, q.From) || !sameBound(snapshot.To, q.To) || snapshot.CheckedAt.IsZero() ||
		snapshot.CheckedAt.Before(s.now().Add(-2*time.Minute)) || snapshot.CheckedAt.After(s.now().Add(time.Minute)) ||
		snapshot.ResolvedTickets < 0 || snapshot.ApprovedReviews < 0 || snapshot.RejectedReviews < 0 {
		return nil, fmt.Errorf("%w: source snapshot scope, window or provenance is invalid", ErrInvalidInput)
	}
	report, err := s.Overview(ctx, q)
	if err != nil {
		return nil, err
	}
	inbox := false
	for _, surface := range q.Surfaces {
		if surface == SurfaceInbox {
			inbox = true
		}
	}
	if !inbox {
		return report, nil
	}
	definitions := []struct {
		metric, label, definition string
		value                     int64
	}{
		{"tickets_currently_resolved", "Resolved tickets", "Distinct retained tickets currently resolved or closed whose latest resolution timestamp falls in the selected window. Personal scope filters the current assignee, not the resolver. Reopened tickets are excluded.", snapshot.ResolvedTickets},
		{"ai_reviews_approved", "AI reviews approved", "Saved approval decisions in Conversation Core during the selected window. Personal scope filters the reviewer. All action types are included.", snapshot.ApprovedReviews},
		{"ai_reviews_rejected", "AI reviews rejected", "Saved rejection decisions in Conversation Core during the selected window. Personal scope filters the reviewer. All action types are included.", snapshot.RejectedReviews},
	}
	for _, d := range definitions {
		report.Scorecards = append(report.Scorecards, Scorecard{ID: "inbox." + d.metric, Label: d.label, Definition: d.definition,
			Surface: SurfaceInbox, Metric: d.metric, Value: float64(d.value), Unit: "count", Source: snapshot.Source,
			EventCount: int(d.value), Aggregation: "owner_record_count", Coverage: "measured", CheckedAt: &snapshot.CheckedAt})
	}
	report.ContractVersion = "2.1"
	report.CoverageDetail = "Support outcomes were verified against Conversation Core's retained records. Other activity series show recorded events; their delivery completeness has not been verified."
	return report, nil
}
