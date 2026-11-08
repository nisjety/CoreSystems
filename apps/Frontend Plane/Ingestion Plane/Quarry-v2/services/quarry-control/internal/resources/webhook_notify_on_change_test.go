package resources

// notifyOnChange filter on webhook subscriptions (E3.5-7): the filter model, the classification the
// edge's change webhook carries, and the filter-aware fan-out. The filter is not persisted yet, so
// the fan-out is driven here with an explicit filterOf (see W23-W3-E3.5-7-status.md).

import (
	"strings"
	"testing"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func TestNotifyFilter_NormalizeNamesTheClosedTypes(t *testing.T) {
	t.Parallel()
	got, reason, _ := normalizeChangeNotifyFilter(ChangeNotifyFilter{
		Types: []string{" removed ", "changed", "changed"}, MinParagraphDelta: 3,
	})
	if reason != "" {
		t.Fatalf("reason = %q", reason)
	}
	if strings.Join(got.Types, ",") != "changed,removed" || got.MinParagraphDelta != 3 {
		t.Fatalf("normalized = %+v", got)
	}

	cases := []struct {
		name   string
		in     ChangeNotifyFilter
		reason string
	}{
		{"no types", ChangeNotifyFilter{}, "notify_types_empty"},
		{"unknown type", ChangeNotifyFilter{Types: []string{"new", "edited"}}, "notify_type_unsupported"},
		{"negative threshold", ChangeNotifyFilter{Types: []string{"changed"}, MinParagraphDelta: -1}, "notify_min_delta_out_of_range"},
		{"threshold above the cap", ChangeNotifyFilter{Types: []string{"changed"}, MinParagraphDelta: maxNotifyMinParagraphDelta + 1}, "notify_min_delta_out_of_range"},
	}
	for _, c := range cases {
		if _, r, _ := normalizeChangeNotifyFilter(c.in); r != c.reason {
			t.Errorf("%s: reason = %q, want %q", c.name, r, c.reason)
		}
	}
	if _, r, _ := normalizeChangeNotifyFilter(ChangeNotifyFilter{Types: []string{"new"}, MinParagraphDelta: maxNotifyMinParagraphDelta}); r != "" {
		t.Fatalf("the cap itself is allowed, got reason %q", r)
	}
}

func TestNotifyFilter_MatchesTypeAndThreshold(t *testing.T) {
	t.Parallel()
	changed := func(delta int) changeNotice { return changeNotice{Type: ChangeTypeChanged, ParagraphDelta: delta} }
	cases := []struct {
		name   string
		filter ChangeNotifyFilter
		change changeNotice
		want   bool
	}{
		{"changed filter takes a change", ChangeNotifyFilter{Types: []string{ChangeTypeChanged}}, changed(0), true},
		{"new-only filter refuses a change", ChangeNotifyFilter{Types: []string{ChangeTypeNew}}, changed(5), false},
		{"removed-only filter refuses a change", ChangeNotifyFilter{Types: []string{ChangeTypeRemoved}}, changed(5), false},
		{"threshold refuses a smaller change", ChangeNotifyFilter{Types: []string{ChangeTypeChanged}, MinParagraphDelta: 3}, changed(2), false},
		{"threshold takes a change at the bound", ChangeNotifyFilter{Types: []string{ChangeTypeChanged}, MinParagraphDelta: 3}, changed(3), true},
		{"markup-only change refused by a threshold", ChangeNotifyFilter{Types: []string{ChangeTypeChanged}, MinParagraphDelta: 1}, changed(0), false},
		{"unknown delta is not dropped", ChangeNotifyFilter{Types: []string{ChangeTypeChanged}, MinParagraphDelta: 3}, changed(-1), true},
		{"threshold does not apply to a new page", ChangeNotifyFilter{Types: []string{ChangeTypeNew}, MinParagraphDelta: 3}, changeNotice{Type: ChangeTypeNew, ParagraphDelta: -1}, true},
	}
	for _, c := range cases {
		if got := c.filter.matches(c.change); got != c.want {
			t.Errorf("%s: matches = %v, want %v", c.name, got, c.want)
		}
	}
}

func TestChangeNotice_ReadsTheEdgesClassification(t *testing.T) {
	t.Parallel()
	cases := []struct {
		name   string
		status quarrycontracts.ChangeStatus
		probe  map[string]any
		want   changeNotice
	}{
		{"edge sends type and delta", quarrycontracts.ChangeChanged,
			map[string]any{"change_type": "changed", "paragraph_delta": float64(4)},
			changeNotice{Type: ChangeTypeChanged, ParagraphDelta: 4}},
		{"older edge: type from the status, delta unknown", quarrycontracts.ChangeChanged,
			map[string]any{}, changeNotice{Type: ChangeTypeChanged, ParagraphDelta: -1}},
		{"null delta is unknown", quarrycontracts.ChangeChanged,
			map[string]any{"paragraph_delta": nil}, changeNotice{Type: ChangeTypeChanged, ParagraphDelta: -1}},
		{"negative delta is unknown", quarrycontracts.ChangeChanged,
			map[string]any{"paragraph_delta": float64(-2)}, changeNotice{Type: ChangeTypeChanged, ParagraphDelta: -1}},
		{"unknown type falls back to the status", quarrycontracts.ChangeNew,
			map[string]any{"change_type": "edited"}, changeNotice{Type: ChangeTypeNew, ParagraphDelta: -1}},
	}
	for _, c := range cases {
		if got := classifyChangeNotice(c.status, c.probe); got != c.want {
			t.Errorf("%s: got %+v, want %+v", c.name, got, c.want)
		}
	}
}

// changeEvent is the change_detected event the edge's webhook produces, reduced to the fields the
// fan-out keys on (the source url and the new baseline).
func changeEvent() quarrycontracts.Event {
	return quarrycontracts.Event{
		EventID: quarrycontracts.NewID(quarrycontracts.KindEvent),
		Type:    quarrycontracts.EvtChangeDetected,
		Payload: map[string]any{
			"source_url":   "https://example.com/pricing",
			"new_baseline": map[string]any{"baseline_id": "bln_01NEW"},
		},
	}
}

// fanoutCase runs one classified change through the fan-out over three subscriptions of org o1 and
// one of org o2, with the filters given, and returns the deliveries per subscription.
func fanoutCase(t *testing.T, change changeNotice) map[string]int {
	t.Helper()
	db := store.NewMemory()
	for _, id := range []string{"whk_all", "whk_changed_big", "whk_new_only"} {
		seedSub(t, db, id, "o1", []string{"change_detected"}, true)
	}
	seedSub(t, db, "whk_other_org", "o2", []string{"change_detected"}, true)
	filters := map[string]*ChangeNotifyFilter{
		"whk_changed_big": {Types: []string{ChangeTypeChanged}, MinParagraphDelta: 3},
		"whk_new_only":    {Types: []string{ChangeTypeNew}},
	}
	filterOf := func(wh store.Webhook) *ChangeNotifyFilter { return filters[string(wh.ID)] }
	fanoutChangeWebhooks(db, changeEvent(), "o1", change, filterOf)
	out := map[string]int{}
	for _, id := range []string{"whk_all", "whk_changed_big", "whk_new_only", "whk_other_org"} {
		out[id] = len(deliveriesFor(db, id))
	}
	return out
}

func TestFanoutChange_FilterDecidesWhatIsDelivered(t *testing.T) {
	t.Parallel()
	cases := []struct {
		name   string
		change changeNotice
		want   map[string]int
	}{
		{"a small change reaches only the unfiltered subscription",
			changeNotice{Type: ChangeTypeChanged, ParagraphDelta: 2},
			map[string]int{"whk_all": 1, "whk_changed_big": 0, "whk_new_only": 0, "whk_other_org": 0}},
		{"an unknown delta reaches the threshold subscription too",
			changeNotice{Type: ChangeTypeChanged, ParagraphDelta: -1},
			map[string]int{"whk_all": 1, "whk_changed_big": 1, "whk_new_only": 0, "whk_other_org": 0}},
		{"a first capture reaches the new-only subscription",
			changeNotice{Type: ChangeTypeNew, ParagraphDelta: -1},
			map[string]int{"whk_all": 1, "whk_changed_big": 0, "whk_new_only": 1, "whk_other_org": 0}},
	}
	for _, c := range cases {
		got := fanoutCase(t, c.change)
		for id, want := range c.want {
			if got[id] != want {
				t.Errorf("%s: %s got %d deliveries, want %d", c.name, id, got[id], want)
			}
		}
	}
}

// TestFanoutChange_OneChangeIsDeliveredOnceUnderAFilter: the same change reached again by the other
// path (a new event id, the same url and baseline) is still one delivery for the subscription.
func TestFanoutChange_OneChangeIsDeliveredOnceUnderAFilter(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	seedSub(t, db, "whk_big", "o1", []string{"change_detected"}, true)
	filterOf := func(store.Webhook) *ChangeNotifyFilter {
		return &ChangeNotifyFilter{Types: []string{ChangeTypeChanged}, MinParagraphDelta: 1}
	}
	change := changeNotice{Type: ChangeTypeChanged, ParagraphDelta: 2}
	ev := changeEvent()
	fanoutChangeWebhooks(db, ev, "o1", change, filterOf)
	ev.EventID = quarrycontracts.NewID(quarrycontracts.KindEvent)
	fanoutChangeWebhooks(db, ev, "o1", change, filterOf)
	if got := len(deliveriesFor(db, "whk_big")); got != 1 {
		t.Fatalf("one change produced %d deliveries under a filter, want 1", got)
	}
}
