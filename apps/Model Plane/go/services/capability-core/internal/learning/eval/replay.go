package eval

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/sessionreview"
	"google.golang.org/grpc"
	"google.golang.org/protobuf/types/known/structpb"
)

// Outcome labels for an attempt that did not end in a skip reason.
const (
	// OutcomeError is a review that failed and will be redelivered.
	OutcomeError = "error"
	// OutcomeLimitReached is a review the live spend budget refused to start.
	OutcomeLimitReached = "limit_reached"
)

// ReplayModel is the review model a deterministic replay names. The model is a
// fake, so the name only has to be set.
const ReplayModel = "eval-replay"

// errBudgetExhausted is returned for a live model call the spend budget refuses.
// Replay reports the attempt as OutcomeLimitReached.
var errBudgetExhausted = errors.New("eval: live spend budget exhausted")

// inferer is the inference-core call a review makes.
type inferer interface {
	Infer(ctx context.Context, in *mpv1.InferRequest, opts ...grpc.CallOption) (*mpv1.InferResponse, error)
}

// Result is what one replayed case did.
type Result struct {
	// Outcomes holds one entry per attempt: the skip reason, or "" for a review
	// that ran, OutcomeError, or OutcomeLimitReached.
	Outcomes []string
	// ModelCalls counts the calls the review made to the model, including any the
	// live budget refused.
	ModelCalls int
	// TranscriptReads counts the conversation reads across the attempts.
	TranscriptReads int
	// Owners are the owners of the replayed runs.
	Owners map[string]bool
	// Proposals are the skills the review wrote, in order.
	Proposals []*mpv1.UpsertAgentSkillRequest
}

// Replay runs every attempt of a case through [sessionreview.ReviewRunCompleted]
// and returns what happened. model is the inference client; nil replays the
// case's own scripted reply. Each call builds fresh fakes, so replays are
// independent of one another.
func Replay(ctx context.Context, f Fixture, model inferer, reviewModel string) (Result, error) {
	if model == nil {
		model = &scripted{reply: f.ModelReply, stop: f.ModelStopReason}
	}
	msgs, err := messages(f.Conversation)
	if err != nil {
		return Result{}, fmt.Errorf("eval: %s: conversation: %w", f.ID, err)
	}
	sc := &session{messages: msgs, skills: skills(f.ExistingSkills)}
	calls := &metered{inner: model}
	opts := []sessionreview.Option{sessionreview.WithMemoryControls(&answer{controls: &mpv1.MemoryControls{
		LearningAllowed: f.Controls.LearningAllowed,
		Reason:          f.Controls.Reason,
	}})}
	if f.Cost != nil {
		budget, err := budgetFor(f.Cost.Budget)
		if err != nil {
			return Result{}, fmt.Errorf("eval: %s: %w", f.ID, err)
		}
		opts = append(opts, sessionreview.WithCostControls(sessionreview.CostControls{
			MinUserTurns:       f.Cost.MinUserTurns,
			MaxTranscriptChars: f.Cost.MaxTranscriptChars,
			MaxPerUserPerDay:   f.Cost.MaxPerUserPerDay,
			MaxPerOrgPerDay:    f.Cost.MaxPerOrgPerDay,
			Budget:             budget,
		}))
	}

	res := Result{Owners: map[string]bool{}}
	for _, a := range f.Attempts {
		owner := a.UserID
		if owner == "" {
			owner = f.Run.UserID
		}
		res.Owners[owner] = true
		data, err := f.envelope(owner)
		if err != nil {
			return Result{}, fmt.Errorf("eval: %s: envelope: %w", f.ID, err)
		}
		outcome, err := sessionreview.ReviewRunCompleted(ctx, data, sc, calls, reviewModel, opts...)
		switch {
		case err == nil:
			res.Outcomes = append(res.Outcomes, string(outcome.Skipped))
		case errors.Is(err, errBudgetExhausted):
			res.Outcomes = append(res.Outcomes, OutcomeLimitReached)
		default:
			res.Outcomes = append(res.Outcomes, OutcomeError)
		}
	}
	res.ModelCalls = calls.calls
	res.TranscriptReads = sc.reads
	res.Proposals = sc.upserts
	return res, nil
}

// envelope builds the RUN_COMPLETED a run for owner publishes. The zdr flag is
// written only when the case declares it, so an absent flag stays absent.
func (f Fixture) envelope(owner string) ([]byte, error) {
	eventType := f.Run.EventType
	if eventType == "" {
		eventType = sessionreview.RunCompletedEventType
	}
	fields := map[string]any{
		"event_type":   eventType,
		"org_id":       f.Run.OrgID,
		"user_id":      owner,
		"resource_ref": "run/" + f.Run.RunID,
		"payload":      map[string]string{"thread_id": f.Run.ThreadID},
	}
	if f.Run.ZDR != nil {
		fields["zdr"] = *f.Run.ZDR
	}
	return json.Marshal(fields)
}

// Check compares a replayed case with its expectations and returns one line per
// mismatch; none means the case holds. A live run (live true) lets the model decide
// how many proposals a case yields, within the case's bounds. The shape of every
// proposal is forced by the gates, so it is checked in both modes.
func Check(f Fixture, r Result, live bool) []string {
	var bad []string
	if !sameStrings(r.Outcomes, f.Expect.Outcomes) {
		bad = append(bad, fmt.Sprintf("outcomes: want %v, got %v", f.Expect.Outcomes, r.Outcomes))
	}
	if r.ModelCalls != f.Expect.ModelCalls {
		bad = append(bad, fmt.Sprintf("model calls: want %d, got %d", f.Expect.ModelCalls, r.ModelCalls))
	}
	if r.TranscriptReads != f.Expect.TranscriptReads {
		bad = append(bad, fmt.Sprintf("transcript reads: want %d, got %d", f.Expect.TranscriptReads, r.TranscriptReads))
	}
	n := len(r.Proposals)
	if live && f.Live != nil {
		if n < f.Live.Min || n > f.Live.Max {
			bad = append(bad, fmt.Sprintf("proposals: want %d to %d, got %d", f.Live.Min, f.Live.Max, n))
		}
	} else if n != f.Expect.ProposalCount {
		bad = append(bad, fmt.Sprintf("proposals: want %d, got %d", f.Expect.ProposalCount, n))
	}
	for i, p := range r.Proposals {
		if p.GetOwnerUserId() == "" || !r.Owners[p.GetOwnerUserId()] {
			bad = append(bad, fmt.Sprintf("proposal %d: owner %q is not the owner of a replayed run", i+1, p.GetOwnerUserId()))
		}
		want := f.Expect.Proposal
		if want == nil {
			continue
		}
		if p.GetScope() != want.Scope {
			bad = append(bad, fmt.Sprintf("proposal %d: scope %q, want %q", i+1, p.GetScope(), want.Scope))
		}
		if p.GetEnabled() != want.Enabled {
			bad = append(bad, fmt.Sprintf("proposal %d: enabled %t, want %t", i+1, p.GetEnabled(), want.Enabled))
		}
		if p.GetReviewState() != want.ReviewState {
			bad = append(bad, fmt.Sprintf("proposal %d: review_state %q, want %q", i+1, p.GetReviewState(), want.ReviewState))
		}
		if p.GetOrigin() != want.Origin {
			bad = append(bad, fmt.Sprintf("proposal %d: origin %q, want %q", i+1, p.GetOrigin(), want.Origin))
		}
	}
	return bad
}

// Summary renders a result as one stable line, so two replays of a case can be
// compared for determinism.
func (r Result) Summary() string {
	var b strings.Builder
	fmt.Fprintf(&b, "%q|%d|%d", r.Outcomes, r.ModelCalls, r.TranscriptReads)
	for _, p := range r.Proposals {
		fmt.Fprintf(&b, "|%s/%s/%s/%t/%s/%s", p.GetName(), p.GetOwnerUserId(), p.GetScope(), p.GetEnabled(), p.GetReviewState(), p.GetOrigin())
	}
	return b.String()
}

func sameStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// messages turns the case's turns into the session-core messages the review reads.
func messages(turns []Turn) ([]*mpv1.SessionMessage, error) {
	out := make([]*mpv1.SessionMessage, 0, len(turns))
	for _, t := range turns {
		m := &mpv1.SessionMessage{Role: t.Role, Content: t.Content}
		if t.SourceScope != "" {
			meta, err := structpb.NewStruct(map[string]any{"source_scope": t.SourceScope})
			if err != nil {
				return nil, err
			}
			m.Metadata = meta
		}
		out = append(out, m)
	}
	return out, nil
}

func skills(list []Skill) []*mpv1.AgentSkill {
	out := make([]*mpv1.AgentSkill, 0, len(list))
	for _, s := range list {
		out = append(out, &mpv1.AgentSkill{Name: s.Name, Origin: s.Origin})
	}
	return out
}

func budgetFor(kind string) (sessionreview.ReviewBudget, error) {
	switch kind {
	case "memory":
		return &memoryBudget{}, nil
	case "failing":
		return &memoryBudget{fail: true}, nil
	case "absent":
		return nil, nil
	}
	return nil, fmt.Errorf("unknown budget %q", kind)
}

// session is the session-core client a review reads and writes. It records what
// the review read and what it wrote.
type session struct {
	messages []*mpv1.SessionMessage
	skills   []*mpv1.AgentSkill
	reads    int
	upserts  []*mpv1.UpsertAgentSkillRequest
}

func (s *session) ListConversation(_ context.Context, _ *mpv1.ListConversationRequest, _ ...grpc.CallOption) (*mpv1.ListConversationResponse, error) {
	s.reads++
	return &mpv1.ListConversationResponse{Messages: s.messages}, nil
}

func (s *session) ListAgentSkills(_ context.Context, _ *mpv1.ListAgentSkillsRequest, _ ...grpc.CallOption) (*mpv1.ListAgentSkillsResponse, error) {
	return &mpv1.ListAgentSkillsResponse{Skills: s.skills}, nil
}

func (s *session) UpsertAgentSkill(_ context.Context, in *mpv1.UpsertAgentSkillRequest, _ ...grpc.CallOption) (*mpv1.UpsertAgentSkillResponse, error) {
	s.upserts = append(s.upserts, in)
	return &mpv1.UpsertAgentSkillResponse{Created: true}, nil
}

// answer is the memory-controls answer the review asks for each run.
type answer struct {
	controls *mpv1.MemoryControls
}

func (a *answer) GetMemoryControls(_ context.Context, _ *mpv1.GetMemoryControlsRequest, _ ...grpc.CallOption) (*mpv1.GetMemoryControlsResponse, error) {
	return &mpv1.GetMemoryControlsResponse{Controls: a.controls}, nil
}

// scripted answers every review with the case's own reply.
type scripted struct {
	reply string
	stop  string
}

func (s *scripted) Infer(_ context.Context, _ *mpv1.InferRequest, _ ...grpc.CallOption) (*mpv1.InferResponse, error) {
	return &mpv1.InferResponse{Content: s.reply, StopReason: s.stop}, nil
}

// metered counts the model calls a review makes.
type metered struct {
	inner inferer
	calls int
}

func (m *metered) Infer(ctx context.Context, in *mpv1.InferRequest, opts ...grpc.CallOption) (*mpv1.InferResponse, error) {
	m.calls++
	return m.inner.Infer(ctx, in, opts...)
}

// memoryBudget counts reviews in memory. fail makes every increment error, as an
// unreachable counter store does.
type memoryBudget struct {
	counts map[string]int64
	fail   bool
}

func (b *memoryBudget) Incr(_ context.Context, key string, _ time.Duration) (int64, error) {
	if b.fail {
		return 0, errors.New("eval: counter store unreachable")
	}
	if b.counts == nil {
		b.counts = map[string]int64{}
	}
	b.counts[key]++
	return b.counts[key], nil
}
