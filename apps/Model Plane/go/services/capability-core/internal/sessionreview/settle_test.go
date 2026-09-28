package sessionreview

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/nats-io/nats.go"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/structpb"
)

// fakeDelivery records how settleReview settled one JetStream message.
type fakeDelivery struct {
	delivered  uint64
	calls      []string // in order: in_progress, ack, nak, term
	nakDelay   time.Duration
	inProgress time.Time
}

func (f *fakeDelivery) Metadata() (*nats.MsgMetadata, error) {
	return &nats.MsgMetadata{NumDelivered: f.delivered, Sequence: nats.SequencePair{Stream: 42}}, nil
}
func (f *fakeDelivery) Ack(...nats.AckOpt) error { f.calls = append(f.calls, "ack"); return nil }
func (f *fakeDelivery) NakWithDelay(delay time.Duration, _ ...nats.AckOpt) error {
	f.calls = append(f.calls, "nak")
	f.nakDelay = delay
	return nil
}
func (f *fakeDelivery) Term(...nats.AckOpt) error { f.calls = append(f.calls, "term"); return nil }
func (f *fakeDelivery) InProgress(...nats.AckOpt) error {
	f.calls = append(f.calls, "in_progress")
	f.inProgress = time.Now()
	return nil
}

// countingInfClient counts model calls and answers like inference-core does
// for a forced tool call; with block set it instead waits for the call's
// context, returning the status gRPC would.
type countingInfClient struct {
	calls       int
	block       bool
	args        string
	gotDeadline time.Time
}

func (f *countingInfClient) Infer(ctx context.Context, _ *mpv1.InferRequest, _ ...grpc.CallOption) (*mpv1.InferResponse, error) {
	f.calls++
	f.gotDeadline, _ = ctx.Deadline()
	if f.block {
		<-ctx.Done()
		return nil, status.FromContextError(ctx.Err()).Err()
	}
	return &mpv1.InferResponse{
		StopReason: "tool_use",
		ToolCalls:  []*mpv1.ToolCall{{Name: "submit_skill_review", ArgumentsJson: f.args}},
	}, nil
}

// captureLogs routes slog to a buffer for the rest of the test.
func captureLogs(t *testing.T) *bytes.Buffer {
	t.Helper()
	var buf bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewJSONHandler(&buf, nil)))
	t.Cleanup(func() { slog.SetDefault(previous) })
	return &buf
}

// logRecords decodes the captured JSON log lines.
func logRecords(t *testing.T, buf *bytes.Buffer) []map[string]any {
	t.Helper()
	var out []map[string]any
	for _, line := range strings.Split(strings.TrimSpace(buf.String()), "\n") {
		if line == "" {
			continue
		}
		var rec map[string]any
		if err := json.Unmarshal([]byte(line), &rec); err != nil {
			t.Fatalf("log line is not JSON: %q", line)
		}
		out = append(out, rec)
	}
	return out
}

func recordsWithMsg(records []map[string]any, msg string) []map[string]any {
	var out []map[string]any
	for _, rec := range records {
		if rec["msg"] == msg {
			out = append(out, rec)
		}
	}
	return out
}

func reviewableConversation() *mpv1.ListConversationResponse {
	return &mpv1.ListConversationResponse{Messages: []*mpv1.SessionMessage{
		{Role: "user", Content: "how do I build the edge image?"},
		{Role: "assistant", Content: "use the build target"},
	}}
}

func TestHandleRunCompleted_ConversationOnlyNeverCallsModelAndLogsSkip(t *testing.T) {
	logs := captureLogs(t)
	scope, _ := structpb.NewStruct(map[string]interface{}{"source_scope": "conversation"})
	sc := &fakeSessionClient{
		convo: &mpv1.ListConversationResponse{Messages: []*mpv1.SessionMessage{
			{Role: "user", Content: "Fictional customer and prices", Metadata: scope},
			{Role: "assistant", Content: "Here is the quote."},
		}},
		skills: &mpv1.ListAgentSkillsResponse{},
	}
	ic := &countingInfClient{args: `{"skills":[]}`}
	msg := &fakeDelivery{delivered: 1}
	data := envBytes(t, "RUN_COMPLETED", "run/r-scope", "org-7", `{"thread_id":"t-scope"}`)

	settleReview(context.Background(), msg, "mp.v1.run.r-scope.event", defaultReviewLimits(),
		func(ctx context.Context) (learning.ReviewOutcome, error) {
			outcome, err := handleRunCompleted(ctx, data, sc, ic, "m")
			if err != nil || outcome != (learning.ReviewOutcome{Skipped: learning.SkipConversationScope}) {
				t.Fatalf("outcome = %+v err=%v, want conversation_scope skip", outcome, err)
			}
			return outcome, err
		})

	if ic.calls != 0 {
		t.Fatalf("model called %d time(s) for a conversation-only thread", ic.calls)
	}
	if got := strings.Join(msg.calls, ","); got != "in_progress,ack" {
		t.Fatalf("delivery settled as %q, want in_progress,ack", got)
	}
	records := logRecords(t, logs)
	skipped := recordsWithMsg(records, "learning review skipped")
	if len(skipped) != 1 || skipped[0]["reason"] != string(learning.SkipConversationScope) || skipped[0]["run_id"] != "r-scope" {
		t.Fatalf("want one conversation_scope skip log, got %v", records)
	}
	if completed := recordsWithMsg(records, "learning review completed"); len(completed) != 0 {
		t.Fatalf("a skipped run must not log as completed: %v", completed)
	}
	if strings.Contains(logs.String(), "Fictional customer") {
		t.Fatal("transcript content reached the logs")
	}
}

func TestHandleRunCompleted_CompletedLogCarriesCountsAndNoContent(t *testing.T) {
	logs := captureLogs(t)
	sc := &fakeSessionClient{convo: reviewableConversation(), skills: &mpv1.ListAgentSkillsResponse{}}
	content := "Steps:\n```bash\ndocker build -f deploy/compose/Dockerfile.edge --target build .\n```\nThen verify."
	args, _ := json.Marshal(map[string]any{"skills": []any{
		map[string]any{"name": "Edge build", "description": "d", "content": content, "trigger_keywords": []string{"docker"}, "confidence": 0.8},
		map[string]any{"name": "Weak", "description": "d", "content": "maybe", "trigger_keywords": []string{}, "confidence": 0.1},
	}})
	ic := &countingInfClient{args: string(args)}

	n, err := HandleRunCompleted(context.Background(),
		envBytes(t, "RUN_COMPLETED", "run/r-done", "org-7", `{"thread_id":"t-done"}`), sc, ic, "m")
	if err != nil || n != 1 || len(sc.upserts) != 1 || sc.upserts[0].GetContent() != content {
		t.Fatalf("fenced skill content must persist intact: n=%d err=%v upserts=%+v", n, err, sc.upserts)
	}
	completed := recordsWithMsg(logRecords(t, logs), "learning review completed")
	if len(completed) != 1 {
		t.Fatalf("want one completed log, got %s", logs.String())
	}
	for key, want := range map[string]float64{"candidates": 2, "selected": 1, "skills_persisted": 1} {
		if completed[0][key] != want {
			t.Fatalf("completed log %s = %v, want %v", key, completed[0][key], want)
		}
	}
	if strings.Contains(logs.String(), "Edge build") || strings.Contains(logs.String(), "docker build") {
		t.Fatal("generated skill content reached the logs")
	}
}

func TestSettleReview_BlockingModelHitsDeadlineBeforeAckWaitAndIsRetried(t *testing.T) {
	captureLogs(t)
	limits := reviewLimits{ackWait: 600 * time.Millisecond, maxDeliver: 5}
	sc := &fakeSessionClient{convo: reviewableConversation(), skills: &mpv1.ListAgentSkillsResponse{}}
	ic := &countingInfClient{block: true}
	msg := &fakeDelivery{delivered: 1}
	data := envBytes(t, "RUN_COMPLETED", "run/r-slow", "org-7", `{"thread_id":"t-slow"}`)

	start := time.Now()
	settleReview(context.Background(), msg, "mp.v1.run.r-slow.event", limits,
		func(ctx context.Context) (learning.ReviewOutcome, error) {
			return handleRunCompleted(ctx, data, sc, ic, "m")
		})
	elapsed := time.Since(start)

	if ic.calls != 1 || ic.gotDeadline.IsZero() {
		t.Fatalf("the model call must run under a deadline (calls=%d)", ic.calls)
	}
	if window := ic.gotDeadline.Sub(msg.inProgress); window >= limits.ackWait {
		t.Fatalf("review deadline %s after the ack-wait restart; it must fall inside the %s ack wait", window, limits.ackWait)
	}
	if elapsed >= limits.ackWait {
		t.Fatalf("review settled after %s, not before the %s ack wait", elapsed, limits.ackWait)
	}
	if got := strings.Join(msg.calls, ","); got != "in_progress,nak" {
		t.Fatalf("a timed-out review must be retried with delay, not terminated or acked: %q", got)
	}
	if msg.nakDelay != reviewRetryDelay(1) {
		t.Fatalf("nak delay = %s, want %s", msg.nakDelay, reviewRetryDelay(1))
	}
	if len(sc.upserts) != 0 {
		t.Fatalf("a timed-out review persisted %d skill(s)", len(sc.upserts))
	}
}

func TestSettleReview_PermanentFailureTerminatesAndExhaustionFollowsMaxDeliver(t *testing.T) {
	captureLogs(t)
	fail := func(err error) func(context.Context) (learning.ReviewOutcome, error) {
		return func(context.Context) (learning.ReviewOutcome, error) { return learning.ReviewOutcome{}, err }
	}
	permanent := &fakeDelivery{delivered: 1}
	settleReview(context.Background(), permanent, "s", defaultReviewLimits(), fail(status.Error(codes.PermissionDenied, "denied")))
	if got := strings.Join(permanent.calls, ","); got != "in_progress,term" {
		t.Fatalf("permanent failure settled as %q, want term", got)
	}

	limits := reviewLimits{ackWait: time.Minute, maxDeliver: 3}
	transient := errors.New("llmreviewer: invalid_schema")
	retried := &fakeDelivery{delivered: 2}
	settleReview(context.Background(), retried, "s", limits, fail(transient))
	if got := strings.Join(retried.calls, ","); got != "in_progress,nak" {
		t.Fatalf("delivery 2 of 3 settled as %q, want nak", got)
	}
	last := &fakeDelivery{delivered: 3}
	settleReview(context.Background(), last, "s", limits, fail(transient))
	if got := strings.Join(last.calls, ","); got != "in_progress" {
		t.Fatalf("the consumer's last delivery must stay unsettled for recovery, got %q", got)
	}
	unlimited := &fakeDelivery{delivered: 50}
	settleReview(context.Background(), unlimited, "s", reviewLimits{ackWait: time.Minute, maxDeliver: -1}, fail(transient))
	if got := strings.Join(unlimited.calls, ","); got != "in_progress,nak" {
		t.Fatalf("with no MaxDeliver a failure is always retried, got %q", got)
	}
}

func TestReviewLimits_FollowTheBoundConsumer(t *testing.T) {
	defaults := defaultReviewLimits()
	if defaults.deadline() != 100*time.Second || defaults.deadline() >= defaults.ackWait {
		t.Fatalf("default review deadline = %s, want 100s inside the %s ack wait", defaults.deadline(), defaults.ackWait)
	}
	if !defaults.exhausted(5) || defaults.exhausted(4) {
		t.Fatal("the default limit must match the provisioned MaxDeliver of 5")
	}

	got := limitsFromConsumer(&nats.ConsumerInfo{Config: nats.ConsumerConfig{AckWait: 60 * time.Second, MaxDeliver: 8}})
	if got.ackWait != 60*time.Second || got.maxDeliver != 8 || got.deadline() != 40*time.Second {
		t.Fatalf("limits not taken from the consumer: %+v deadline=%s", got, got.deadline())
	}
	if short := (reviewLimits{ackWait: 30 * time.Second}); short.deadline() != 15*time.Second {
		t.Fatalf("a short ack wait must still leave half of it: %s", short.deadline())
	}
	if got := limitsFromConsumer(nil); got != defaults {
		t.Fatalf("missing info must keep the defaults: %+v", got)
	}
}
