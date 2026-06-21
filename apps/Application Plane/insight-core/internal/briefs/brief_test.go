package briefs

import (
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

func overview(orgID string, surfaceTotals map[string]int) *insights.Overview {
	ov := &insights.Overview{OrgID: orgID}
	for surface, total := range surfaceTotals {
		ov.Surfaces = append(ov.Surfaces, insights.SurfaceOverview{Surface: surface, TotalEvents: total})
	}
	return ov
}

func TestAssembleBrief_PreviewBelowGate(t *testing.T) {
	b := AssembleBrief(overview("org-1", map[string]int{insights.SurfaceInbox: 2}))
	if b.State != StatePreview {
		t.Fatalf("state = %q, want %q", b.State, StatePreview)
	}
	if b.Disclosure != PreviewDisclosure {
		t.Errorf("preview brief must carry the disclosure; got %q", b.Disclosure)
	}
	if b.TotalEvents != 2 {
		t.Errorf("total = %d, want 2 (real count only)", b.TotalEvents)
	}
	payload := b.Payload()
	if payload["state"] != StatePreview || payload["preview"] != true {
		t.Errorf("payload must expose the Preview gate: %+v", payload)
	}
	if payload["disclosure"] != PreviewDisclosure {
		t.Errorf("payload must carry the disclosure for in_app/email render")
	}
}

func TestAssembleBrief_LiveAtOrAboveGate(t *testing.T) {
	b := AssembleBrief(overview("org-1", map[string]int{
		insights.SurfaceInbox:  3,
		insights.SurfaceSocial: 2,
	}))
	if b.State != StateLive {
		t.Fatalf("state = %q, want %q (total %d >= gate %d)", b.State, StateLive, b.TotalEvents, BriefMinEvents)
	}
	if b.TotalEvents != 5 {
		t.Errorf("total = %d, want 5", b.TotalEvents)
	}
	if b.Disclosure != "" {
		t.Errorf("live brief must not carry a preview disclosure; got %q", b.Disclosure)
	}
	if b.Payload()["preview"] != false {
		t.Errorf("live brief payload preview flag must be false")
	}
}

func TestAssembleBrief_NilOverviewIsHonestEmptyPreview(t *testing.T) {
	b := AssembleBrief(nil)
	if b.State != StatePreview {
		t.Fatalf("nil overview must be a preview, got %q", b.State)
	}
	if b.TotalEvents != 0 {
		t.Errorf("nil overview must yield zero events, never fabricated; got %d", b.TotalEvents)
	}
	if b.Disclosure != PreviewDisclosure {
		t.Errorf("empty preview must carry the disclosure")
	}
}

func TestAssembleBrief_NoFabricatedCounts(t *testing.T) {
	// Surfaces with zero events are dropped, not surfaced as activity.
	b := AssembleBrief(overview("org-1", map[string]int{
		insights.SurfaceInbox:  0,
		insights.SurfaceSocial: 0,
	}))
	if b.TotalEvents != 0 {
		t.Errorf("zero-event surfaces must not inflate the total; got %d", b.TotalEvents)
	}
	if len(b.Surfaces) != 0 {
		t.Errorf("zero-event surfaces must be dropped; got %+v", b.Surfaces)
	}
}
