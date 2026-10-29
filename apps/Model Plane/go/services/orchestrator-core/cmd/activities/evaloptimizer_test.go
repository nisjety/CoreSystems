package activities_test

import (
	"context"
	"net"
	"sync"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/evaloptimizer"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
)

// ─── fake inference-core ─────────────────────────────────────────────────────

type fakeInferenceCore struct {
	mpv1.UnimplementedInferenceCoreServer

	mu        sync.Mutex
	responses []*mpv1.InferResponse
	errs      []error
	requests  []*mpv1.InferRequest
	idx       int
}

func (f *fakeInferenceCore) Infer(_ context.Context, req *mpv1.InferRequest) (*mpv1.InferResponse, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.requests = append(f.requests, req)
	i := f.idx
	f.idx++
	if i < len(f.errs) && f.errs[i] != nil {
		return nil, f.errs[i]
	}
	if i >= len(f.responses) {
		return nil, status.Error(codes.Internal, "fake inference-core: no scripted response")
	}
	return f.responses[i], nil
}

func (f *fakeInferenceCore) recorded() []*mpv1.InferRequest {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]*mpv1.InferRequest, len(f.requests))
	copy(out, f.requests)
	return out
}

func newInferenceCoreConn(t *testing.T, fake *fakeInferenceCore) *grpc.ClientConn {
	t.Helper()
	lis := bufconn.Listen(bufSize)
	srv := grpc.NewServer()
	mpv1.RegisterInferenceCoreServer(srv, fake)
	go func() { _ = srv.Serve(lis) }()
	t.Cleanup(srv.GracefulStop)

	conn, err := grpc.NewClient(
		"passthrough://bufnet",
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) {
			return lis.DialContext(ctx)
		}),
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	require.NoError(t, err)
	t.Cleanup(func() { _ = conn.Close() })
	return conn
}

func inferResp(content string, in, out int32) *mpv1.InferResponse {
	return &mpv1.InferResponse{Content: content, InputTokens: in, OutputTokens: out, ModelUsed: "test-model"}
}

func baseInput() activities.EvalOptimizerInput {
	return activities.EvalOptimizerInput{
		RunID:           "run-eo-1",
		ThreadID:        "thread-1",
		OrgID:           "org-1",
		UserID:          "user-1",
		GeneratorModel:  "claude-sonnet-5",
		GeneratorSystem: "be helpful",
		Task:            "write a limerick",
		JudgeModel:      "claude-haiku-4-5",
		JudgeSystem:     "grade limericks",
		Rubric:          "five lines, AABBA rhyme",
		MaxRounds:       3,
	}
}

// ─── activity behaviour ──────────────────────────────────────────────────────

func TestEvaluatorOptimizerActivity_NilConnUnavailable(t *testing.T) {
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{InferenceCore: nil})
	_, err := a.EvaluatorOptimizerActivity(context.Background(), baseInput())
	require.Error(t, err)
	assert.Equal(t, codes.Unavailable, status.Code(err))
}

func TestEvaluatorOptimizerActivity_PassesFirstRound(t *testing.T) {
	fake := &fakeInferenceCore{responses: []*mpv1.InferResponse{
		inferResp("a poem", 20, 10),
		inferResp(`{"passed": true, "score": 0.95, "feedback": "great"}`, 15, 5),
	}}
	conn := newInferenceCoreConn(t, fake)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{InferenceCore: conn})

	out, err := a.EvaluatorOptimizerActivity(context.Background(), baseInput())
	require.NoError(t, err)

	assert.True(t, out.Passed)
	assert.Equal(t, evaloptimizer.StopPassed, out.StopReason)
	assert.Equal(t, 1, out.RoundsRun)
	assert.Equal(t, "a poem", out.BestAnswer)
	assert.InDelta(t, 0.95, out.BestScore, 1e-9)
	assert.Equal(t, 50, out.TotalTokens) // 20+10 + 15+5

	reqs := fake.recorded()
	require.Len(t, reqs, 2)
	// Generator leg then judge leg, distinct models, both org-scoped.
	assert.Equal(t, "claude-sonnet-5", reqs[0].Model)
	assert.Equal(t, "claude-haiku-4-5", reqs[1].Model)
	for i, r := range reqs {
		assert.Equalf(t, "org-1", r.OrgId, "req %d must carry org scope", i)
	}
	// The judge leg requests the structured verdict schema; the generator does not.
	assert.Empty(t, reqs[0].StructuredOutputSchema)
	assert.Equal(t, evaloptimizer.VerdictSchema, reqs[1].StructuredOutputSchema)
}

func TestEvaluatorOptimizerActivity_LoopsThenPasses(t *testing.T) {
	fake := &fakeInferenceCore{responses: []*mpv1.InferResponse{
		inferResp("draft 1", 10, 10),
		inferResp(`{"passed": false, "score": 0.4, "feedback": "fix the rhyme"}`, 10, 5),
		inferResp("draft 2", 10, 10),
		inferResp(`{"passed": true, "score": 0.9, "feedback": "good"}`, 10, 5),
	}}
	conn := newInferenceCoreConn(t, fake)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{InferenceCore: conn})

	out, err := a.EvaluatorOptimizerActivity(context.Background(), baseInput())
	require.NoError(t, err)

	assert.True(t, out.Passed)
	assert.Equal(t, 2, out.RoundsRun)
	assert.Equal(t, "draft 2", out.BestAnswer)

	reqs := fake.recorded()
	require.Len(t, reqs, 4)
	// The second generator round (req[2]) must carry the judge's feedback.
	var genRound2 string
	for _, m := range reqs[2].Messages {
		genRound2 += m.Content + "\n"
	}
	assert.Contains(t, genRound2, "fix the rhyme")
	assert.Contains(t, genRound2, "draft 1")
}

func TestEvaluatorOptimizerActivity_ZDRPropagatesToProvider(t *testing.T) {
	fake := &fakeInferenceCore{responses: []*mpv1.InferResponse{
		inferResp("x", 1, 1),
		inferResp(`{"passed": false, "score": 0.1, "feedback": "no"}`, 1, 1),
		inferResp("y", 1, 1),
		inferResp(`{"passed": true, "score": 1, "feedback": "ok"}`, 1, 1),
	}}
	conn := newInferenceCoreConn(t, fake)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{InferenceCore: conn})

	in := baseInput()
	in.ZDR = true
	_, err := a.EvaluatorOptimizerActivity(context.Background(), in)
	require.NoError(t, err)

	reqs := fake.recorded()
	require.NotEmpty(t, reqs)
	for i, r := range reqs {
		assert.Truef(t, r.Zdr, "infer request #%d must set ZDR so a ZDR run never reaches a retaining provider", i)
	}
}

func TestEvaluatorOptimizerActivity_HitsCapDidNotPass(t *testing.T) {
	fake := &fakeInferenceCore{responses: []*mpv1.InferResponse{
		inferResp("try 1", 1, 1),
		inferResp(`{"passed": false, "score": 0.3, "feedback": "again"}`, 1, 1),
		inferResp("try 2", 1, 1),
		inferResp(`{"passed": false, "score": 0.6, "feedback": "closer"}`, 1, 1),
	}}
	conn := newInferenceCoreConn(t, fake)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{InferenceCore: conn})

	in := baseInput()
	in.MaxRounds = 2
	out, err := a.EvaluatorOptimizerActivity(context.Background(), in)
	require.NoError(t, err)

	assert.False(t, out.Passed)
	assert.Equal(t, evaloptimizer.StopMaxRounds, out.StopReason)
	assert.Equal(t, 2, out.RoundsRun)
	assert.Equal(t, "try 2", out.BestAnswer) // 0.6 > 0.3
}

func TestEvaluatorOptimizerActivity_ProviderErrorPropagates(t *testing.T) {
	fake := &fakeInferenceCore{
		responses: []*mpv1.InferResponse{nil},
		errs:      []error{status.Error(codes.Unavailable, "provider down")},
	}
	conn := newInferenceCoreConn(t, fake)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{InferenceCore: conn})

	out, err := a.EvaluatorOptimizerActivity(context.Background(), baseInput())
	require.Error(t, err)
	assert.NotEqual(t, evaloptimizer.StopPassed, out.StopReason)
}
