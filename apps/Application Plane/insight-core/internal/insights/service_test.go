package insights

import (
	"context"
	"strings"
	"testing"
	"time"
)

func TestOverviewIncludesAllVerevonSurfaces(t *testing.T) {
	now := time.Date(2026, 6, 16, 9, 0, 0, 0, time.UTC)
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})), WithNow(func() time.Time {
		return now
	}))

	if _, err := service.RecordMetricEvent(context.Background(), IngestMetricEventInput{
		OrgID:      "org-1",
		Surface:    SurfaceSocial,
		Metric:     "published_posts",
		Value:      3,
		OccurredAt: now.Add(-time.Hour),
	}); err != nil {
		t.Fatalf("RecordMetricEvent error: %v", err)
	}

	overview, err := service.Overview(context.Background(), OverviewQuery{OrgID: "org-1"})
	if err != nil {
		t.Fatalf("Overview error: %v", err)
	}
	if overview.Plane.ServicePlane != ServicePlane {
		t.Fatalf("plane = %s, want %s", overview.Plane.ServicePlane, ServicePlane)
	}
	if len(overview.Surfaces) != len(SupportedSurfaces()) {
		t.Fatalf("surface count = %d, want %d", len(overview.Surfaces), len(SupportedSurfaces()))
	}
	if overview.Surfaces[0].Surface != SurfaceSocial || overview.Surfaces[0].Metrics[0].Value != 3 {
		t.Fatalf("social rollup = %#v, want published_posts=3", overview.Surfaces[0])
	}
}

func TestOverviewCitesRealEventSourcesNeverFabricates(t *testing.T) {
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})))

	// Two events on the same metric from the same real producer.
	for i := 0; i < 2; i++ {
		if _, err := service.RecordMetricEvent(context.Background(), IngestMetricEventInput{
			OrgID:   "org-1",
			Surface: SurfaceInbox,
			Metric:  "ai_actions_executed",
			Value:   1,
			Source:  "conversation-core",
		}); err != nil {
			t.Fatalf("RecordMetricEvent error: %v", err)
		}
	}
	// A metric with NO source must stay unattributed (never fabricated).
	if _, err := service.RecordMetricEvent(context.Background(), IngestMetricEventInput{
		OrgID:   "org-1",
		Surface: SurfaceInbox,
		Metric:  "tickets_created",
		Value:   1,
	}); err != nil {
		t.Fatalf("RecordMetricEvent error: %v", err)
	}

	overview, err := service.Overview(context.Background(), OverviewQuery{OrgID: "org-1", Surfaces: []string{SurfaceInbox}})
	if err != nil {
		t.Fatalf("Overview error: %v", err)
	}

	var executed, ticketsCreated *Scorecard
	for i := range overview.Scorecards {
		switch overview.Scorecards[i].Metric {
		case "ai_actions_executed":
			executed = &overview.Scorecards[i]
		case "tickets_created":
			ticketsCreated = &overview.Scorecards[i]
		}
	}
	if executed == nil || ticketsCreated == nil {
		t.Fatalf("missing scorecards: executed=%v ticketsCreated=%v", executed != nil, ticketsCreated != nil)
	}
	if executed.Source != "conversation-core" {
		t.Errorf("scorecard source = %q, want %q (real producer citation)", executed.Source, "conversation-core")
	}
	if executed.Value != 2 {
		t.Errorf("scorecard value = %v, want 2", executed.Value)
	}
	if ticketsCreated.Source != "" {
		t.Errorf("unattributed metric must have an empty source, never fabricated; got %q", ticketsCreated.Source)
	}
}

// TestOverviewRelabelsPilotHeadlineScorecards locks the "Conversations
// handled" / "AI drafts approved" / "AI drafts rejected" presentation labels
// to their exact real metric ids — relabeling never changes the recorded
// value or fabricates a new one.
func TestOverviewRelabelsPilotHeadlineScorecards(t *testing.T) {
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})))
	events := []IngestMetricEventInput{
		{OrgID: "org-1", Surface: SurfaceInbox, Metric: "tickets_resolved", Value: 5, Source: "conversation-core"},
		{OrgID: "org-1", Surface: SurfaceInbox, Metric: "ai_actions_approved", Value: 3, Source: "conversation-core"},
		{OrgID: "org-1", Surface: SurfaceInbox, Metric: "ai_actions_rejected", Value: 1, Source: "conversation-core"},
		// Unlisted metric must keep the generic transform, not a fabricated label.
		{OrgID: "org-1", Surface: SurfaceInbox, Metric: "tickets_created", Value: 2, Source: "conversation-core"},
	}
	for _, event := range events {
		if _, err := service.RecordMetricEvent(context.Background(), event); err != nil {
			t.Fatalf("RecordMetricEvent error: %v", err)
		}
	}

	overview, err := service.Overview(context.Background(), OverviewQuery{OrgID: "org-1", Surfaces: []string{SurfaceInbox}})
	if err != nil {
		t.Fatalf("Overview error: %v", err)
	}

	labels := map[string]string{}
	values := map[string]float64{}
	for _, card := range overview.Scorecards {
		labels[card.ID] = card.Label
		values[card.ID] = card.Value
	}

	wantLabels := map[string]string{
		"inbox.tickets_resolved":    "Conversations handled",
		"inbox.ai_actions_approved": "AI drafts approved",
		"inbox.ai_actions_rejected": "AI drafts rejected",
		"inbox.tickets_created":     "inbox tickets created",
	}
	for id, wantLabel := range wantLabels {
		if got := labels[id]; got != wantLabel {
			t.Errorf("label[%s] = %q, want %q", id, got, wantLabel)
		}
	}
	if values["inbox.tickets_resolved"] != 5 {
		t.Errorf("relabeling changed the value: got %v, want 5", values["inbox.tickets_resolved"])
	}
}

func TestOverviewJoinsMultipleDistinctSourcesSorted(t *testing.T) {
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})))
	for _, source := range []string{"social-core", "conversation-core"} {
		if _, err := service.RecordMetricEvent(context.Background(), IngestMetricEventInput{
			OrgID:   "org-1",
			Surface: SurfaceSocial,
			Metric:  "cross_producer_metric",
			Value:   1,
			Source:  source,
		}); err != nil {
			t.Fatalf("RecordMetricEvent error: %v", err)
		}
	}

	overview, err := service.Overview(context.Background(), OverviewQuery{OrgID: "org-1", Surfaces: []string{SurfaceSocial}})
	if err != nil {
		t.Fatalf("Overview error: %v", err)
	}
	if len(overview.Surfaces[0].Metrics) != 1 {
		t.Fatalf("metrics = %#v, want one rolled-up metric", overview.Surfaces[0].Metrics)
	}
	if got := overview.Surfaces[0].Metrics[0].Source; got != "conversation-core, social-core" {
		t.Errorf("distinct sources = %q, want sorted-joined %q", got, "conversation-core, social-core")
	}
}

func TestOverviewDoesNotLeakEventsAcrossOrganizations(t *testing.T) {
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})))
	for _, orgID := range []string{"org-1", "org-2"} {
		if _, err := service.RecordMetricEvent(context.Background(), IngestMetricEventInput{
			OrgID:   orgID,
			Surface: SurfaceInbox,
			Metric:  "open_conversations",
			Value:   10,
		}); err != nil {
			t.Fatalf("RecordMetricEvent(%s) error: %v", orgID, err)
		}
	}

	overview, err := service.Overview(context.Background(), OverviewQuery{OrgID: "org-1", Surfaces: []string{SurfaceInbox}})
	if err != nil {
		t.Fatalf("Overview error: %v", err)
	}
	if got := overview.Surfaces[0].Metrics[0].Value; got != 10 {
		t.Fatalf("org-scoped value = %v, want 10", got)
	}
}

func TestOverviewNarrowsAnOrganizationReadToTheVerifiedActor(t *testing.T) {
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})))
	for _, event := range []IngestMetricEventInput{
		{OrgID: "org-1", ActorUserID: "user-a", Surface: SurfaceInbox, Metric: "tickets_resolved", Value: 2},
		{OrgID: "org-1", ActorUserID: "user-b", Surface: SurfaceInbox, Metric: "tickets_resolved", Value: 5},
		// Inbound activity has no authenticated human actor. It must stay in the
		// organization roll-up and must not appear in a "my activity" result.
		{OrgID: "org-1", Surface: SurfaceInbox, Metric: "messages_received", Value: 9},
	} {
		if _, err := service.RecordMetricEvent(context.Background(), event); err != nil {
			t.Fatalf("RecordMetricEvent error: %v", err)
		}
	}

	overview, err := service.Overview(context.Background(), OverviewQuery{
		OrgID:       "org-1",
		ActorUserID: "user-a",
		Surfaces:    []string{SurfaceInbox},
	})
	if err != nil {
		t.Fatalf("Overview error: %v", err)
	}
	if len(overview.Scorecards) != 1 {
		t.Fatalf("scorecards = %#v, want only user-a activity", overview.Scorecards)
	}
	if got := overview.Scorecards[0].Value; got != 2 {
		t.Fatalf("actor-scoped value = %v, want 2", got)
	}
}

func TestOverviewFiltersByWindowAndSurfaceList(t *testing.T) {
	now := time.Date(2026, 6, 16, 9, 0, 0, 0, time.UTC)
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})), WithNow(func() time.Time {
		return now
	}))
	events := []IngestMetricEventInput{
		{OrgID: "org-1", Surface: SurfaceSocial, Metric: "published_posts", Value: 1, OccurredAt: now.Add(-2 * time.Hour)},
		{OrgID: "org-1", Surface: SurfaceSocial, Metric: "published_posts", Value: 2, OccurredAt: now.Add(-30 * time.Minute)},
		{OrgID: "org-1", Surface: SurfaceInbox, Metric: "open_conversations", Value: 4, OccurredAt: now.Add(-30 * time.Minute)},
	}
	for _, event := range events {
		if _, err := service.RecordMetricEvent(context.Background(), event); err != nil {
			t.Fatalf("RecordMetricEvent error: %v", err)
		}
	}

	from := now.Add(-time.Hour)
	overview, err := service.Overview(context.Background(), OverviewQuery{
		OrgID:    "org-1",
		Surfaces: []string{"social,inbox"},
		From:     &from,
	})
	if err != nil {
		t.Fatalf("Overview error: %v", err)
	}
	if got := overview.Surfaces[0].Metrics[0].Value; got != 2 {
		t.Fatalf("filtered social value = %v, want 2", got)
	}
	if got := overview.Surfaces[1].Metrics[0].Value; got != 4 {
		t.Fatalf("filtered inbox value = %v, want 4", got)
	}
}

func TestRejectsUnsupportedSurface(t *testing.T) {
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})))
	_, err := service.RecordMetricEvent(context.Background(), IngestMetricEventInput{
		OrgID:   "org-1",
		Surface: "billing",
		Metric:  "usage",
		Value:   1,
	})
	if !IsInvalidInput(err) {
		t.Fatalf("error = %v, want invalid input", err)
	}
}

func TestOverviewRejectsInvalidWindow(t *testing.T) {
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})))
	from := time.Date(2026, 6, 17, 0, 0, 0, 0, time.UTC)
	to := time.Date(2026, 6, 16, 0, 0, 0, 0, time.UTC)

	_, err := service.Overview(context.Background(), OverviewQuery{
		OrgID: "org-1",
		From:  &from,
		To:    &to,
	})
	if !IsInvalidInput(err) {
		t.Fatalf("error = %v, want invalid input", err)
	}
}

func TestListConnectorSlotsRequiresOrg(t *testing.T) {
	service := NewService(NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{})))

	_, err := service.ListConnectorSlots(context.Background(), "")
	if !IsInvalidInput(err) {
		t.Fatalf("error = %v, want invalid input", err)
	}
}

func TestOwnershipDecisionKeepsInsightsOutOfControlPlaneStorage(t *testing.T) {
	decision := OwnershipDecision()
	if decision.ServicePlane != "application-plane" {
		t.Fatalf("service plane = %s, want application-plane", decision.ServicePlane)
	}
	joined := strings.Join(decision.Rules, " ")
	for _, required := range []string{"org scope", "integration-core", "no direct Control"} {
		if !strings.Contains(joined, required) {
			t.Fatalf("ownership decision missing %q in %q", required, joined)
		}
	}
}

func TestDefaultConnectorsIncludeGoogleContractsWithoutSecrets(t *testing.T) {
	connectors := DefaultConnectorSlots(ConnectorSlotOptions{TokenLeaseAudience: "insight-core"})
	var ga4, searchConsole *ConnectorSlot
	for i := range connectors {
		switch connectors[i].Type {
		case "google_analytics_4":
			ga4 = &connectors[i]
		case "google_search_console":
			searchConsole = &connectors[i]
		}
	}
	if ga4 == nil || searchConsole == nil {
		t.Fatalf("missing google connector slots: ga4=%v searchConsole=%v", ga4 != nil, searchConsole != nil)
	}
	if ga4.Status != ConnectorStatusRequiresTokenLease || searchConsole.Status != ConnectorStatusRequiresTokenLease {
		t.Fatalf("google connector status must require token lease")
	}
	for _, connector := range []ConnectorSlot{*ga4, *searchConsole} {
		for _, env := range connector.RequiredEnv {
			if strings.Contains(strings.ToLower(env), "secret") {
				t.Fatalf("connector %s declares secret env %s", connector.Type, env)
			}
		}
	}
}
