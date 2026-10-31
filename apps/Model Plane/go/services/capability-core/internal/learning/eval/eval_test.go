package eval

import (
	"context"
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// proposalFor is a proposal as the sink writes it, with the given shape.
func proposalFor(owner, scope string, enabled bool, state, origin string) *mpv1.UpsertAgentSkillRequest {
	return &mpv1.UpsertAgentSkillRequest{
		Name:        "Weekly report",
		OwnerUserId: owner,
		Scope:       scope,
		Enabled:     enabled,
		ReviewState: state,
		Origin:      origin,
	}
}

// requiredCategories are the control categories the seeded set must cover. Each
// one names a way a review must not learn, or a shape a proposal must keep.
var requiredCategories = []string{
	"recurring",
	"one_off",
	"prompt_injection",
	"pii",
	"paused_user",
	"org_disabled",
	"zdr",
	"owner",
	"space_origin",
	"cost_controls",
	"model_failure",
}

func loadSeeded(t *testing.T) []Fixture {
	t.Helper()
	cases, err := Load(SeededDir)
	if err != nil {
		t.Fatalf("load seeded cases: %v", err)
	}
	return cases
}

// TestSeededCasesReplayToTheirExpectations runs every seeded case through the
// production review path, twice, and holds each run to its expectations.
func TestSeededCasesReplayToTheirExpectations(t *testing.T) {
	for _, f := range loadSeeded(t) {
		t.Run(f.ID, func(t *testing.T) {
			if f.Upstream != "" {
				t.Logf("covered upstream by %s", f.Upstream)
				return
			}
			if f.Gap != "" {
				t.Logf("control gap: %s", f.Gap)
			}
			first, err := Replay(context.Background(), f, nil, ReplayModel)
			if err != nil {
				t.Fatalf("replay: %v", err)
			}
			for _, bad := range Check(f, first, false) {
				t.Error(bad)
			}
			second, err := Replay(context.Background(), f, nil, ReplayModel)
			if err != nil {
				t.Fatalf("second replay: %v", err)
			}
			if first.Summary() != second.Summary() {
				t.Errorf("replay is not deterministic:\n first  %s\n second %s", first.Summary(), second.Summary())
			}
		})
	}
}

// TestSeededCasesCoverEveryControlCategory fails when a control has no case, so a
// gate cannot be removed without a case noticing.
func TestSeededCasesCoverEveryControlCategory(t *testing.T) {
	seen := map[string]int{}
	ids := map[string]bool{}
	for _, f := range loadSeeded(t) {
		seen[f.Category]++
		if ids[f.ID] {
			t.Errorf("duplicate case id %q", f.ID)
		}
		ids[f.ID] = true
	}
	for _, category := range requiredCategories {
		if seen[category] == 0 {
			t.Errorf("no seeded case for category %q", category)
		}
	}
}

// TestSeededCasesThatReachTheModelHaveLiveBounds keeps the live run honest: every
// case that the model answers must say how many proposals a live model may make.
func TestSeededCasesThatReachTheModelHaveLiveBounds(t *testing.T) {
	for _, f := range loadSeeded(t) {
		if f.Upstream != "" || f.Scripted || f.Expect.ModelCalls == 0 {
			continue
		}
		if f.Live == nil {
			t.Errorf("case %s reaches the model but has no live bounds", f.ID)
		}
	}
}

// TestValidateRefusesCasesThatSayNothing checks the shape rules the loader
// enforces, one broken case at a time.
func TestValidateRefusesCasesThatSayNothing(t *testing.T) {
	ok := Fixture{
		ID:       "shape-check",
		Category: "recurring",
		Attempts: []Attempt{{}},
		Expect:   Expect{Outcomes: []string{""}, ModelCalls: 1},
		Live:     &Bounds{Min: 0, Max: 1},
	}
	if err := validate(ok); err != nil {
		t.Fatalf("a well-formed case must pass: %v", err)
	}
	cases := map[string]func(f *Fixture){
		"bad id":           func(f *Fixture) { f.ID = "Not_Kebab" },
		"no category":      func(f *Fixture) { f.Category = "" },
		"no attempts":      func(f *Fixture) { f.Attempts = nil; f.Expect.Outcomes = nil },
		"outcome mismatch": func(f *Fixture) { f.Expect.Outcomes = []string{"", ""} },
		"no live bounds":   func(f *Fixture) { f.Live = nil },
		"inverted bounds":  func(f *Fixture) { f.Live = &Bounds{Min: 2, Max: 1} },
		"unknown budget":   func(f *Fixture) { f.Cost = &CostFixture{Budget: "shared"} },
		"upstream replays": func(f *Fixture) { f.Upstream = "session-core" },
	}
	for name, break_ := range cases {
		f := ok
		f.Attempts = append([]Attempt(nil), ok.Attempts...)
		f.Expect.Outcomes = append([]string(nil), ok.Expect.Outcomes...)
		break_(&f)
		if err := validate(f); err == nil {
			t.Errorf("%s: must be refused", name)
		}
	}
}

// TestCheckHoldsTheShapeOfEveryProposal proves Check reports a proposal the gates
// would never write: an enabled one, or one owned by a stranger.
func TestCheckHoldsTheShapeOfEveryProposal(t *testing.T) {
	f := Fixture{
		ID:       "shape-check",
		Category: "prompt_injection",
		Attempts: []Attempt{{}},
		Run:      Run{UserID: "user-1"},
		Expect: Expect{
			Outcomes:        []string{""},
			ModelCalls:      1,
			TranscriptReads: 1,
			ProposalCount:   1,
			Proposal:        &Proposal{Scope: "user", Enabled: false, ReviewState: "proposed", Origin: "background_review"},
		},
	}
	good := Result{
		Outcomes:        []string{""},
		ModelCalls:      1,
		TranscriptReads: 1,
		Owners:          map[string]bool{"user-1": true},
	}
	good.Proposals = append(good.Proposals, proposalFor("user-1", "user", false, "proposed", "background_review"))
	if bad := Check(f, good, false); len(bad) != 0 {
		t.Fatalf("a well-formed proposal must hold: %v", bad)
	}
	enabled := good
	enabled.Proposals = []*mpv1.UpsertAgentSkillRequest{proposalFor("user-1", "org", true, "accepted", "user")}
	if bad := Check(f, enabled, false); len(bad) == 0 {
		t.Fatal("an enabled org-wide proposal must be reported")
	}
	stranger := good
	stranger.Proposals = []*mpv1.UpsertAgentSkillRequest{proposalFor("user-9", "user", false, "proposed", "background_review")}
	if bad := Check(f, stranger, false); len(bad) == 0 {
		t.Fatal("a proposal owned by someone who did not run must be reported")
	}
	if !strings.Contains(good.Summary(), "user-1") {
		t.Fatalf("the summary must name the owner: %s", good.Summary())
	}
}
