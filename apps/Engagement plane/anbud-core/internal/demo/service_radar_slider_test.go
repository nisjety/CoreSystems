package demo

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/incumbency"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/rawstore"
)

// radarAward is an award of the given CPV code, made in June 2025 for 24 months: it ends in June 2027,
// inside the radar's horizon from the test clock (10 November 2026).
func radarAward(n int, cpv string) rawstore.Document {
	return doc(fmt.Sprintf("2025-%06d", 300000+n), eformstest.AwardWithResult(eformstest.Notice{
		ID:          fmt.Sprintf("00000000-0000-4000-8000-%012d", 300000+n),
		ProcedureID: fmt.Sprintf("00000000-0000-4000-8000-%012d", 400000+n),
		CPV:         cpv, BuyerOrg: "974760673",
	}, eformstest.Award{
		WinnerName: "Fiktiv Leverandør AS", WinnerOrg: "111111111", Tenders: 3,
		AwardDate: time.Date(2025, time.June, 1, 0, 0, 0, 0, time.UTC), DurationMonths: 24,
	}))
}

// radarCorpus has the profile's own code, a code in the same group, a code in the same division only,
// a code of another division, and a division that covers the profile's code.
func radarCorpus() []rawstore.Document {
	return []rawstore.Document{
		radarAward(1, "72212000"),
		radarAward(2, "72215000"),
		radarAward(3, "72300000"),
		radarAward(4, "45000000"),
		radarAward(5, "72000000"),
	}
}

func decodeRadar(t *testing.T, body string) ReTenderRequest {
	t.Helper()
	r, errs := DecodeReTenderRequest([]byte(body))
	if len(errs) != 0 {
		t.Fatalf("DecodeReTenderRequest(%s): %+v", body, errs)
	}
	return r
}

func radarIDs(results []AwardView) []string {
	out := make([]string, 0, len(results))
	for _, r := range results {
		out = append(out, r.NoticeID)
	}
	return out
}

// At strict the radar lists only the profile's own code and the division that covers it. The slider
// says what each wider step would add: balanced adds the same group, wide adds the same division.
func TestRadarSliderStrictListsOnlyWhatIsInside(t *testing.T) {
	t.Parallel()

	req := decodeRadar(t, `{"profile":{"cpv":["72212000"]},"strictness":"strict"}`)
	got, err := newService(t, &fakeNotices{docs: radarCorpus()}, nil).ReTenders(ctx(), req)
	if err != nil {
		t.Fatalf("ReTenders: %v", err)
	}
	if got.Strictness != "strict" {
		t.Errorf("strictness = %q, want strict", got.Strictness)
	}
	if want := []string{"2025-300001", "2025-300005"}; strings.Join(radarIDs(got.Results), ",") != strings.Join(want, ",") {
		t.Errorf("results = %v, want %v", radarIDs(got.Results), want)
	}
	if len(got.Widening) != 2 || got.Widening[0].To != "balanced" || got.Widening[0].Adds != 1 ||
		got.Widening[1].To != "wide" || got.Widening[1].Adds != 2 {
		t.Errorf("widening = %+v, want balanced adding 1 and wide adding 2", got.Widening)
	}
	// Each wider step names the dials it widens, as the codes the app words.
	wantDials := map[string]string{"balanced": "cpv_group,nuts2_region", "wide": "cpv_division,nuts1_region"}
	for _, w := range got.Widening {
		if dials := strings.Join(w.Dials, ","); dials != wantDials[w.To] {
			t.Errorf("the step to %s names dials %q, want %q", w.To, dials, wantDials[w.To])
		}
	}
}

// Wide is the widest step: the exact matches (the profile's code and the division covering it) come first,
// then the near ones, and nothing is left to widen.
func TestRadarSliderWideListsWhatTheWidestStepReaches(t *testing.T) {
	t.Parallel()

	req := decodeRadar(t, `{"profile":{"cpv":["72212000"]},"strictness":"wide"}`)
	got, err := newService(t, &fakeNotices{docs: radarCorpus()}, nil).ReTenders(ctx(), req)
	if err != nil {
		t.Fatalf("ReTenders: %v", err)
	}
	if want := []string{"2025-300001", "2025-300005", "2025-300002", "2025-300003"}; strings.Join(radarIDs(got.Results), ",") != strings.Join(want, ",") {
		t.Errorf("results = %v, want %v", radarIDs(got.Results), want)
	}
	if got.Strictness != "wide" || len(got.Widening) != 0 {
		t.Errorf("strictness %q widening %+v, want wide and nothing to widen", got.Strictness, got.Widening)
	}
}

// The radar defaults to balanced, as the matches do. Strict is what the radar did before the slider.
func TestRadarSliderDefaultsToBalancedLikeTheList(t *testing.T) {
	t.Parallel()

	req := decodeRadar(t, `{"profile":{"cpv":["72212000"]}}`)
	if req.Scope.Level != match.Balanced {
		t.Errorf("default step = %v, want balanced", req.Scope.Level)
	}
	got, err := newService(t, &fakeNotices{docs: radarCorpus()}, nil).ReTenders(ctx(), req)
	if err != nil {
		t.Fatalf("ReTenders: %v", err)
	}
	if got.Strictness != "balanced" {
		t.Errorf("strictness = %q, want balanced", got.Strictness)
	}
}

// The step is kept once, in the scope: without a profile the answer still names the step the request
// gave, and nothing is left to widen at wide.
func TestRadarSliderTakesTheStepWithoutAProfile(t *testing.T) {
	t.Parallel()

	req := decodeRadar(t, `{"strictness":"wide"}`)
	got, err := newService(t, &fakeNotices{docs: radarCorpus()}, nil).ReTenders(ctx(), req)
	if err != nil {
		t.Fatalf("ReTenders: %v", err)
	}
	if got.Strictness != "wide" || len(got.Widening) != 0 {
		t.Errorf("strictness %q widening %+v, want wide and nothing to widen", got.Strictness, got.Widening)
	}
}

// An unknown step is refused with the field and the allowed values, and the value is not echoed.
func TestRadarSliderRefusesAnUnknownStepWithoutEchoingIt(t *testing.T) {
	t.Parallel()

	_, errs := DecodeReTenderRequest([]byte(`{"strictness":"loose-marker-x"}`))
	if len(errs) != 1 || errs[0].Field != "strictness" || strings.Contains(errs[0].Reason, "loose-marker-x") {
		t.Errorf("errors = %+v, want one error on strictness that does not echo the value", errs)
	}
}

// The list is cut at the cap, and matching says how many matched before the cut, so a cut list is never
// shown as the whole of the radar.
func TestRadarReportsTheTotalBeforeTheCap(t *testing.T) {
	t.Parallel()

	total := incumbency.MaxReTenders + 5
	var docs []rawstore.Document
	for n := 1; n <= total; n++ {
		docs = append(docs, radarAward(n, "72212000"))
	}
	req := decodeRadar(t, `{"profile":{"cpv":["72212000"]},"strictness":"strict"}`)
	got, err := newService(t, &fakeNotices{docs: docs}, nil).ReTenders(ctx(), req)
	if err != nil {
		t.Fatalf("ReTenders: %v", err)
	}
	if len(got.Results) != incumbency.MaxReTenders || got.Matching != total {
		t.Errorf("listed %d of %d matching, want %d listed of %d", len(got.Results), got.Matching, incumbency.MaxReTenders, total)
	}
}

// Each row says whether the strictest step already reaches its codes (exact) or only a wider step does (near),
// so the list can explain why a contract is on it.
func TestRadarMarksEachRowExactOrNear(t *testing.T) {
	t.Parallel()

	req := decodeRadar(t, `{"profile":{"cpv":["72212000"]},"strictness":"balanced"}`)
	got, err := newService(t, &fakeNotices{docs: radarCorpus()}, nil).ReTenders(ctx(), req)
	if err != nil {
		t.Fatalf("ReTenders: %v", err)
	}
	want := []string{"2025-300001:exact", "2025-300005:exact", "2025-300002:near"}
	var marks []string
	for _, r := range got.Results {
		marks = append(marks, r.NoticeID+":"+r.Reach)
	}
	if strings.Join(marks, ",") != strings.Join(want, ",") {
		t.Errorf("rows = %v, want %v", marks, want)
	}
}

// The reach is a field of a radar row only when it is set, so the matches and the audit sheet do not carry it.
func TestAwardViewCarriesTheReachOnlyWhenSet(t *testing.T) {
	t.Parallel()

	with, _ := json.Marshal(AwardView{NoticeID: "n", Reach: "near"})
	without, _ := json.Marshal(AwardView{NoticeID: "n"})
	if !strings.Contains(string(with), `"reach":"near"`) || strings.Contains(string(without), "reach") {
		t.Errorf("with reach %s, without %s", with, without)
	}
}
