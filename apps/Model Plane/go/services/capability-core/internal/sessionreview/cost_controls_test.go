package sessionreview

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"google.golang.org/grpc"
)

// countingInfer answers every review with one reply and counts the model calls,
// which is the spend the cost controls bound.
type countingInfer struct {
	reply string
	calls int
}

func (f *countingInfer) Infer(_ context.Context, _ *mpv1.InferRequest, _ ...grpc.CallOption) (*mpv1.InferResponse, error) {
	f.calls++
	return &mpv1.InferResponse{Content: f.reply}, nil
}

// memBudget is an in-memory ReviewBudget. fail makes every increment error, as an
// unreachable counter store does.
type memBudget struct {
	counts map[string]int64
	keys   []string
	fail   bool
}

func (b *memBudget) Incr(_ context.Context, key string, _ time.Duration) (int64, error) {
	if b.fail {
		return 0, errors.New("counter store unreachable")
	}
	if b.counts == nil {
		b.counts = map[string]int64{}
	}
	b.keys = append(b.keys, key)
	b.counts[key]++
	return b.counts[key], nil
}

func envOf(values map[string]string) func(string) string {
	return func(key string) string { return values[key] }
}

func zdrFlag(v bool) *bool { return &v }

// costEnvelope builds a RUN_COMPLETED owned by owner. A nil zdr leaves the flag
// absent, which fails closed.
func costEnvelope(t *testing.T, owner string, zdr *bool) []byte {
	t.Helper()
	fields := map[string]any{
		"event_type":   RunCompletedEventType,
		"org_id":       "org-1",
		"user_id":      owner,
		"resource_ref": "run/run-" + owner,
		"payload":      map[string]string{"thread_id": "thread-1"},
	}
	if zdr != nil {
		fields["zdr"] = *zdr
	}
	b, err := json.Marshal(fields)
	if err != nil {
		t.Fatalf("marshal envelope: %v", err)
	}
	return b
}

// conversation is a thread of userTurns question-and-answer pairs.
func conversation(userTurns int) *mpv1.ListConversationResponse {
	var msgs []*mpv1.SessionMessage
	for i := 0; i < userTurns; i++ {
		msgs = append(msgs,
			&mpv1.SessionMessage{Role: "user", Content: "weekly report, step one"},
			&mpv1.SessionMessage{Role: "assistant", Content: "done"},
		)
	}
	return &mpv1.ListConversationResponse{Messages: msgs}
}

const costReply = `{"skills":[{"name":"Weekly report","content":"steps","confidence":0.9}]}`

// costRun replays one review with the memory controls allowing learning, the
// given cost controls (nil leaves them off), and returns its outcome.
func costRun(t *testing.T, sc *ownerSessionClient, inf *countingInfer, cost *CostControls, data []byte) learning.ReviewOutcome {
	t.Helper()
	opts := []Option{WithMemoryControls(&fakeControls{controls: &mpv1.MemoryControls{LearningAllowed: true, Reason: "on"}})}
	if cost != nil {
		opts = append(opts, WithCostControls(*cost))
	}
	outcome, err := ReviewRunCompleted(context.Background(), data, sc, inf, "model-x", opts...)
	if err != nil {
		t.Fatalf("review: %v", err)
	}
	return outcome
}

func TestCostControlsStayOffUnlessTheSwitchIsLiteralTrue(t *testing.T) {
	for _, raw := range []string{"", "TRUE", "True", "1", "yes", "on", "true "} {
		_, on, err := CostControlsFromLookup(envOf(map[string]string{learningReviewCostEnv: raw}))
		if err != nil || on {
			t.Fatalf("switch %q must leave the cost controls off (on=%v err=%v)", raw, on, err)
		}
	}
}

func TestCostControlsReadTheirDefaultsAndOverridesWhenOn(t *testing.T) {
	cost, on, err := CostControlsFromLookup(envOf(map[string]string{learningReviewCostEnv: "true"}))
	if err != nil || !on {
		t.Fatalf("switch true must turn the controls on: on=%v err=%v", on, err)
	}
	want := CostControls{MinUserTurns: 3, MaxTranscriptChars: 60000, MaxPerUserPerDay: 5, MaxPerOrgPerDay: 100}
	if cost != want {
		t.Fatalf("defaults: want %+v, got %+v", want, cost)
	}

	cost, _, err = CostControlsFromLookup(envOf(map[string]string{
		learningReviewCostEnv:     "true",
		learningReviewMinTurnsEnv: "2",
		learningReviewUserCapEnv:  "7",
	}))
	if err != nil {
		t.Fatalf("overrides: %v", err)
	}
	if cost.MinUserTurns != 2 || cost.MaxPerUserPerDay != 7 || cost.MaxPerOrgPerDay != 100 {
		t.Fatalf("overrides not applied: %+v", cost)
	}
}

func TestCostControlsIgnoreMalformedBoundsWhileOff(t *testing.T) {
	_, on, err := CostControlsFromLookup(envOf(map[string]string{learningReviewMinTurnsEnv: "nope"}))
	if on || err != nil {
		t.Fatalf("bounds are read only when the switch is on (on=%v err=%v)", on, err)
	}
}

func TestCostControlsRefuseMalformedBoundsWhileOn(t *testing.T) {
	for _, bad := range []string{"0", "-2", "three", "2.5", " 4"} {
		_, _, err := CostControlsFromLookup(envOf(map[string]string{
			learningReviewCostEnv:     "true",
			learningReviewMaxCharsEnv: bad,
		}))
		if err == nil {
			t.Fatalf("bound %q must be refused, not defaulted", bad)
		}
	}
}

func TestWithoutCostControlsAShortConversationIsStillReviewed(t *testing.T) {
	sc := newOwnerSessionClient()
	sc.convo = conversation(1)
	inf := &countingInfer{reply: costReply}
	outcome := costRun(t, sc, inf, nil, costEnvelope(t, "user-1", zdrFlag(false)))
	if outcome.Skipped != "" || inf.calls != 1 || len(sc.upserts) != 1 {
		t.Fatalf("switched off, the review must run as before: skipped=%q calls=%d upserts=%d",
			outcome.Skipped, inf.calls, len(sc.upserts))
	}
}

func TestCostGateSkipsAShortConversationBeforeTheModelIsCalled(t *testing.T) {
	sc := newOwnerSessionClient()
	sc.convo = conversation(2)
	inf := &countingInfer{reply: costReply}
	budget := &memBudget{}
	cost := CostControls{MinUserTurns: 3, MaxTranscriptChars: 60000, MaxPerUserPerDay: 5, MaxPerOrgPerDay: 100, Budget: budget}
	outcome := costRun(t, sc, inf, &cost, costEnvelope(t, "user-1", zdrFlag(false)))
	if outcome.Skipped != skipTooFewTurns {
		t.Fatalf("want %q, got %q", skipTooFewTurns, outcome.Skipped)
	}
	if inf.calls != 0 || sc.transcriptReads != 1 || len(sc.upserts) != 0 {
		t.Fatalf("a short conversation must cost one read and no model call: calls=%d reads=%d upserts=%d",
			inf.calls, sc.transcriptReads, len(sc.upserts))
	}
	if len(budget.keys) != 0 {
		t.Fatalf("a skipped review must not use the owner's cap: %v", budget.keys)
	}
}

func TestCostGateSkipsAnOverlongTranscriptBeforeTheModelIsCalled(t *testing.T) {
	sc := newOwnerSessionClient()
	sc.convo = conversation(3)
	inf := &countingInfer{reply: costReply}
	cost := CostControls{MinUserTurns: 1, MaxTranscriptChars: 20, MaxPerUserPerDay: 5, MaxPerOrgPerDay: 100, Budget: &memBudget{}}
	outcome := costRun(t, sc, inf, &cost, costEnvelope(t, "user-1", zdrFlag(false)))
	if outcome.Skipped != skipTranscriptTooLarge || inf.calls != 0 {
		t.Fatalf("want %q and no model call, got %q with %d calls", skipTranscriptTooLarge, outcome.Skipped, inf.calls)
	}
}

func TestCostGateCapsEachOwnerPerDay(t *testing.T) {
	sc := newOwnerSessionClient()
	sc.convo = conversation(3)
	inf := &countingInfer{reply: costReply}
	cost := CostControls{MinUserTurns: 1, MaxTranscriptChars: 60000, MaxPerUserPerDay: 2, MaxPerOrgPerDay: 100, Budget: &memBudget{}}
	var got []learning.SkipReason
	for i := 0; i < 3; i++ {
		got = append(got, costRun(t, sc, inf, &cost, costEnvelope(t, "user-1", zdrFlag(false))).Skipped)
	}
	want := []learning.SkipReason{"", "", skipUserDailyCap}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("review %d: want %q, got %q (all %v)", i+1, want[i], got[i], got)
		}
	}
	if inf.calls != 2 {
		t.Fatalf("the capped review must not reach the model: calls=%d", inf.calls)
	}
	if other := costRun(t, sc, inf, &cost, costEnvelope(t, "user-2", zdrFlag(false))); other.Skipped != "" {
		t.Fatalf("another owner must not be held back by user-1's cap: %q", other.Skipped)
	}
}

func TestCostGateCapsTheOrganizationPerDay(t *testing.T) {
	sc := newOwnerSessionClient()
	sc.convo = conversation(3)
	inf := &countingInfer{reply: costReply}
	cost := CostControls{MinUserTurns: 1, MaxTranscriptChars: 60000, MaxPerUserPerDay: 5, MaxPerOrgPerDay: 2, Budget: &memBudget{}}
	var got []learning.SkipReason
	for _, owner := range []string{"user-1", "user-2", "user-3"} {
		got = append(got, costRun(t, sc, inf, &cost, costEnvelope(t, owner, zdrFlag(false))).Skipped)
	}
	want := []learning.SkipReason{"", "", skipOrgDailyCap}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("review %d: want %q, got %q (all %v)", i+1, want[i], got[i], got)
		}
	}
	if inf.calls != 2 {
		t.Fatalf("the org-capped review must not reach the model: calls=%d", inf.calls)
	}
}

func TestCostGateCountsOnlyReviewsThatReachTheModel(t *testing.T) {
	sc := newOwnerSessionClient()
	inf := &countingInfer{reply: costReply}
	cost := CostControls{MinUserTurns: 2, MaxTranscriptChars: 60000, MaxPerUserPerDay: 1, MaxPerOrgPerDay: 100, Budget: &memBudget{}}
	sc.convo = conversation(1)
	if got := costRun(t, sc, inf, &cost, costEnvelope(t, "user-1", zdrFlag(false))).Skipped; got != skipTooFewTurns {
		t.Fatalf("the short conversation is skipped as too short, got %q", got)
	}
	sc.convo = conversation(2)
	if got := costRun(t, sc, inf, &cost, costEnvelope(t, "user-1", zdrFlag(false))).Skipped; got != "" {
		t.Fatalf("a skipped conversation must not use up the owner's only review today: got %q", got)
	}
}

func TestCostGateFailsClosedWhenTheCounterIsAbsentOrErrors(t *testing.T) {
	for name, budget := range map[string]ReviewBudget{
		"absent":  nil,
		"failing": &memBudget{fail: true},
	} {
		sc := newOwnerSessionClient()
		sc.convo = conversation(3)
		inf := &countingInfer{reply: costReply}
		cost := CostControls{MinUserTurns: 1, MaxTranscriptChars: 60000, MaxPerUserPerDay: 5, MaxPerOrgPerDay: 100, Budget: budget}
		outcome := costRun(t, sc, inf, &cost, costEnvelope(t, "user-1", zdrFlag(false)))
		if outcome.Skipped != skipBudgetUnavailable || inf.calls != 0 || len(sc.upserts) != 0 {
			t.Fatalf("%s counter: want %q with no model call, got %q, calls=%d upserts=%d",
				name, skipBudgetUnavailable, outcome.Skipped, inf.calls, len(sc.upserts))
		}
	}
}

func TestCostGateNeverReadsOrCountsAZDRRun(t *testing.T) {
	sc := newOwnerSessionClient()
	sc.convo = conversation(3)
	inf := &countingInfer{reply: costReply}
	budget := &memBudget{}
	cost := CostControls{MinUserTurns: 1, MaxTranscriptChars: 60000, MaxPerUserPerDay: 5, MaxPerOrgPerDay: 100, Budget: budget}
	outcome := costRun(t, sc, inf, &cost, costEnvelope(t, "user-1", zdrFlag(true)))
	if outcome.Skipped != skipRetentionPosture || sc.transcriptReads != 0 || len(budget.keys) != 0 || inf.calls != 0 {
		t.Fatalf("a ZDR run is refused before any read or count: skipped=%q reads=%d counted=%v calls=%d",
			outcome.Skipped, sc.transcriptReads, budget.keys, inf.calls)
	}
}
