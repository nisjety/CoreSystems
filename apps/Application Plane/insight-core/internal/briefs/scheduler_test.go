package briefs

import (
	"context"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

type fakeSource struct {
	orgIDs    []string
	overviews map[string]*insights.Overview
	gotOrgIDs []string // org_ids Overview was queried for (cross-tenant guard)
}

func (f *fakeSource) OrgsWithRecentMetrics(_ context.Context, _ time.Duration) ([]string, error) {
	return f.orgIDs, nil
}

func (f *fakeSource) Overview(_ context.Context, query insights.OverviewQuery) (*insights.Overview, error) {
	f.gotOrgIDs = append(f.gotOrgIDs, query.OrgID)
	if ov, ok := f.overviews[query.OrgID]; ok {
		return ov, nil
	}
	return &insights.Overview{OrgID: query.OrgID}, nil
}

type fakeNotifier struct {
	mu   sync.Mutex
	sent []NotificationRequest
}

func (f *fakeNotifier) Send(_ context.Context, req NotificationRequest) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.sent = append(f.sent, req)
	return nil
}

func fixedClock(t time.Time) func() time.Time { return func() time.Time { return t } }

func TestScheduler_DeliversDailyBriefWithPreviewGate(t *testing.T) {
	src := &fakeSource{
		orgIDs: []string{"org-1"},
		overviews: map[string]*insights.Overview{
			"org-1": {OrgID: "org-1", Surfaces: []insights.SurfaceOverview{{Surface: insights.SurfaceAgents, TotalEvents: 2}}},
		},
	}
	notif := &fakeNotifier{}
	day := time.Date(2026, 6, 22, 9, 0, 0, 0, time.UTC)
	sched := NewScheduler(src, notif, WithNow(fixedClock(day)))

	sched.deliverRound(context.Background())

	if len(notif.sent) != 1 {
		t.Fatalf("sent %d, want 1", len(notif.sent))
	}
	req := notif.sent[0]
	if req.Type != WorkflowDailyBrief {
		t.Errorf("type = %q, want %q", req.Type, WorkflowDailyBrief)
	}
	if req.RecipientID != "org-1" {
		t.Errorf("recipient = %q, want server-resolved org-1", req.RecipientID)
	}
	if req.Payload["state"] != StatePreview || req.Payload["preview"] != true {
		t.Errorf("brief must carry the Preview gate; payload=%+v", req.Payload)
	}
	if req.Payload["disclosure"] != PreviewDisclosure {
		t.Errorf("preview brief must carry the disclosure for in_app/email")
	}
	channels, _ := req.Payload["channels"].([]string)
	if len(channels) != 2 || channels[0] != "in_app" || channels[1] != "email" {
		t.Errorf("brief must request in_app+email channels; got %+v", req.Payload["channels"])
	}
}

func TestScheduler_IdempotentPerOrgPerDay(t *testing.T) {
	src := &fakeSource{orgIDs: []string{"org-1"}}
	notif := &fakeNotifier{}
	day := time.Date(2026, 6, 22, 9, 0, 0, 0, time.UTC)
	sched := NewScheduler(src, notif, WithNow(fixedClock(day)))

	// Two rounds on the same UTC day (e.g. a restart) must use the SAME
	// idempotency key so notification-core / Novu dedupe to a single delivery.
	sched.deliverRound(context.Background())
	sched.deliverRound(context.Background())

	if len(notif.sent) != 2 {
		t.Fatalf("sent %d requests, want 2 (dedup is the key, enforced downstream)", len(notif.sent))
	}
	if notif.sent[0].IdempotencyKey == "" || notif.sent[0].IdempotencyKey != notif.sent[1].IdempotencyKey {
		t.Errorf("same-day deliveries must share an idempotency key; got %q and %q",
			notif.sent[0].IdempotencyKey, notif.sent[1].IdempotencyKey)
	}
	want := BriefIdempotencyKey("org-1", "2026-06-22")
	if notif.sent[0].IdempotencyKey != want {
		t.Errorf("idempotency key = %q, want %q", notif.sent[0].IdempotencyKey, want)
	}
}

func TestScheduler_DistinctKeyAcrossDays(t *testing.T) {
	src := &fakeSource{orgIDs: []string{"org-1"}}
	notif := &fakeNotifier{}

	d1 := time.Date(2026, 6, 22, 9, 0, 0, 0, time.UTC)
	NewScheduler(src, notif, WithNow(fixedClock(d1))).deliverRound(context.Background())
	d2 := time.Date(2026, 6, 23, 9, 0, 0, 0, time.UTC)
	NewScheduler(src, notif, WithNow(fixedClock(d2))).deliverRound(context.Background())

	if len(notif.sent) != 2 || notif.sent[0].IdempotencyKey == notif.sent[1].IdempotencyKey {
		t.Errorf("different days must yield different keys; got %q and %q",
			notif.sent[0].IdempotencyKey, notif.sent[1].IdempotencyKey)
	}
}

// TestScheduler_OnlyDeliversToDiscoveredOrgs is the cross-tenant guard: the
// scheduler only ever assembles/delivers for the org_ids server-side discovery
// returned — it never fans out to an org that has no recorded activity, and the
// per-org overview is always scoped to that same org_id (no cross-tenant read).
func TestScheduler_OnlyDeliversToDiscoveredOrgs(t *testing.T) {
	src := &fakeSource{orgIDs: []string{"org-a", "org-b"}}
	notif := &fakeNotifier{}
	day := time.Date(2026, 6, 22, 9, 0, 0, 0, time.UTC)
	sched := NewScheduler(src, notif, WithNow(fixedClock(day)))

	sched.deliverRound(context.Background())

	if len(notif.sent) != 2 {
		t.Fatalf("sent %d, want 2 (one per discovered org)", len(notif.sent))
	}
	// Every overview read and every recipient is one of the discovered orgs.
	allowed := map[string]bool{"org-a": true, "org-b": true}
	for _, got := range src.gotOrgIDs {
		if !allowed[got] {
			t.Errorf("overview queried for non-discovered org %q (cross-tenant)", got)
		}
	}
	for _, req := range notif.sent {
		if !allowed[req.RecipientID] {
			t.Errorf("delivered to non-discovered recipient %q (cross-tenant)", req.RecipientID)
		}
		if req.Payload["org_id"] != req.RecipientID {
			t.Errorf("brief org_id %v must match recipient %q", req.Payload["org_id"], req.RecipientID)
		}
	}
}
