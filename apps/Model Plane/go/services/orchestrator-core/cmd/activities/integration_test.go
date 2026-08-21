package activities_test

import (
	"context"
	"log/slog"
	"net"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
	"google.golang.org/protobuf/types/known/timestamppb"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
)

const bufSize = 1 << 20

// ─── fake servers ────────────────────────────────────────────────────────────

type fakeCapabilityCore struct {
	mpv1.UnimplementedCapabilityCoreServer

	validateResp *mpv1.ValidateSkillBundleResponse
	validateErr  error

	checkResp *mpv1.CheckSkillPromotionResponse
	checkErr  error
}

func (f *fakeCapabilityCore) ValidateSkillBundle(
	_ context.Context,
	_ *mpv1.ValidateSkillBundleRequest,
) (*mpv1.ValidateSkillBundleResponse, error) {
	return f.validateResp, f.validateErr
}

func (f *fakeCapabilityCore) CheckSkillPromotion(
	_ context.Context,
	_ *mpv1.CheckSkillPromotionRequest,
) (*mpv1.CheckSkillPromotionResponse, error) {
	return f.checkResp, f.checkErr
}

// No PromoteSkill override: capability-core removed that RPC (SKILL-2), and
// orchestrator-core no longer calls it (UpdateRegistryActivity was removed
// too), so fakeCapabilityCore falls back to the embedded
// UnimplementedCapabilityCoreServer default like the real server does.

type fakeMemoryService struct {
	mpv1.UnimplementedMemoryServiceServer

	searchResp *mpv1.SearchMemoryResponse
	searchErr  error

	indexErr error
}

type fakeSessionCore struct {
	mpv1.UnimplementedSessionCoreServer

	startScheduledRunResp *mpv1.StartRunResponse
	startScheduledRunErr  error

	startScheduledRunReq *mpv1.StartScheduledRunRequest
}

type fakeExecutionCore struct {
	mpv1.UnimplementedExecutionCoreServer

	executeScheduledStepResp *mpv1.ExecuteScheduledStepResponse
	executeScheduledStepErr  error
	executeScheduledStepReq  *mpv1.ExecuteScheduledStepRequest
}

func (f *fakeExecutionCore) ExecuteScheduledStep(
	_ context.Context,
	req *mpv1.ExecuteScheduledStepRequest,
) (*mpv1.ExecuteScheduledStepResponse, error) {
	f.executeScheduledStepReq = req
	return f.executeScheduledStepResp, f.executeScheduledStepErr
}

func (f *fakeSessionCore) StartScheduledRun(
	_ context.Context,
	req *mpv1.StartScheduledRunRequest,
) (*mpv1.StartRunResponse, error) {
	f.startScheduledRunReq = req
	return f.startScheduledRunResp, f.startScheduledRunErr
}

func (f *fakeMemoryService) SearchMemory(
	_ context.Context,
	_ *mpv1.SearchMemoryRequest,
) (*mpv1.SearchMemoryResponse, error) {
	return f.searchResp, f.searchErr
}

func (f *fakeMemoryService) IndexMemory(
	_ context.Context,
	_ *mpv1.IndexMemoryRequest,
) (*mpv1.IndexMemoryResponse, error) {
	return &mpv1.IndexMemoryResponse{}, f.indexErr
}

// ─── bufconn helpers ─────────────────────────────────────────────────────────

func newCapabilityCoreConn(t *testing.T, fake *fakeCapabilityCore) *grpc.ClientConn {
	t.Helper()
	lis := bufconn.Listen(bufSize)
	srv := grpc.NewServer()
	mpv1.RegisterCapabilityCoreServer(srv, fake)
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

func newMemoryServiceConn(t *testing.T, fake *fakeMemoryService) *grpc.ClientConn {
	t.Helper()
	lis := bufconn.Listen(bufSize)
	srv := grpc.NewServer()
	mpv1.RegisterMemoryServiceServer(srv, fake)
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

func newSessionCoreConn(t *testing.T, fake *fakeSessionCore) *grpc.ClientConn {
	t.Helper()
	lis := bufconn.Listen(bufSize)
	srv := grpc.NewServer()
	mpv1.RegisterSessionCoreServer(srv, fake)
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

func newExecutionCoreConn(t *testing.T, fake *fakeExecutionCore) *grpc.ClientConn {
	t.Helper()
	lis := bufconn.Listen(bufSize)
	srv := grpc.NewServer()
	mpv1.RegisterExecutionCoreServer(srv, fake)
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

type fakeScheduledRunExecutionAuthorizer struct {
	token string
	err   error

	intent activities.ScheduledRunExecutionIntent
	calls  int
}

type fakeScheduledStepDecisionAuthorizer struct {
	token  string
	err    error
	intent activities.ScheduledStepExecutionIntent
	calls  int
}

func (a *fakeScheduledStepDecisionAuthorizer) AuthorizeScheduledStep(
	_ context.Context,
	intent activities.ScheduledStepExecutionIntent,
) (string, error) {
	a.calls++
	a.intent = intent
	return a.token, a.err
}

func (a *fakeScheduledRunExecutionAuthorizer) AuthorizeScheduledRunExecution(
	_ context.Context,
	intent activities.ScheduledRunExecutionIntent,
) (string, error) {
	a.calls++
	a.intent = intent
	return a.token, a.err
}

func testLogger() *slog.Logger {
	return slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelError}))
}

// ─── ValidateSkillBundleActivity ─────────────────────────────────────────────

func TestValidateSkillBundleActivity(t *testing.T) {
	t.Run("valid bundle", func(t *testing.T) {
		fake := &fakeCapabilityCore{
			validateResp: &mpv1.ValidateSkillBundleResponse{Valid: true, Errors: nil},
		}
		conn := newCapabilityCoreConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{CapabilityCore: conn})

		out, err := a.ValidateSkillBundleActivity(
			context.Background(),
			activities.SkillValidationInput{SkillID: "skill-123"},
		)
		require.NoError(t, err)
		assert.True(t, out.Valid)
		assert.Empty(t, out.Errors)
	})

	t.Run("invalid bundle with errors", func(t *testing.T) {
		fake := &fakeCapabilityCore{
			validateResp: &mpv1.ValidateSkillBundleResponse{
				Valid:  false,
				Errors: []string{"missing manifest", "bad schema"},
			},
		}
		conn := newCapabilityCoreConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{CapabilityCore: conn})

		out, err := a.ValidateSkillBundleActivity(
			context.Background(),
			activities.SkillValidationInput{SkillID: "skill-bad"},
		)
		require.NoError(t, err)
		assert.False(t, out.Valid)
		assert.Equal(t, []string{"missing manifest", "bad schema"}, out.Errors)
	})

	t.Run("nil capability-core conn returns unavailable error", func(t *testing.T) {
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{CapabilityCore: nil})
		_, err := a.ValidateSkillBundleActivity(
			context.Background(),
			activities.SkillValidationInput{SkillID: "skill-nil"},
		)
		require.Error(t, err)
		assert.Equal(t, codes.Unavailable, status.Code(err))
	})
}

// ─── RunPromotionGateActivity ─────────────────────────────────────────────────

func TestRunPromotionGateActivity(t *testing.T) {
	t.Run("gate passed", func(t *testing.T) {
		fake := &fakeCapabilityCore{
			checkResp: &mpv1.CheckSkillPromotionResponse{
				Passed: true,
				Checks: []string{"lint-ok", "tests-ok"},
			},
		}
		conn := newCapabilityCoreConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{CapabilityCore: conn})

		out, err := a.RunPromotionGateActivity(
			context.Background(),
			activities.PromotionGateInput{SkillID: "s1", FromScope: "dev", ToScope: "staging"},
		)
		require.NoError(t, err)
		assert.True(t, out.Passed)
		assert.Equal(t, []string{"lint-ok", "tests-ok"}, out.Checks)
	})

	t.Run("gate failed", func(t *testing.T) {
		fake := &fakeCapabilityCore{
			checkResp: &mpv1.CheckSkillPromotionResponse{
				Passed: false,
				Checks: []string{"tests-failed"},
			},
		}
		conn := newCapabilityCoreConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{CapabilityCore: conn})

		out, err := a.RunPromotionGateActivity(
			context.Background(),
			activities.PromotionGateInput{SkillID: "s1", FromScope: "dev", ToScope: "staging"},
		)
		require.NoError(t, err)
		assert.False(t, out.Passed)
		assert.Equal(t, []string{"tests-failed"}, out.Checks)
	})

	t.Run("nil conn returns error", func(t *testing.T) {
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{CapabilityCore: nil})
		_, err := a.RunPromotionGateActivity(
			context.Background(),
			activities.PromotionGateInput{SkillID: "s1", FromScope: "dev", ToScope: "staging"},
		)
		require.Error(t, err)
		assert.Equal(t, codes.Unavailable, status.Code(err))
	})
}

// UpdateRegistryActivity and its tests were removed per SKILL-2: the activity
// called capability-core's now-removed PromoteSkill RPC. See activities.go's
// comment where the type used to be defined, and
// workflows.ErrRegistryUpdateNotSupported for what replaced it.

// ─── QueryMemoryEntriesActivity ───────────────────────────────────────────────

func TestQueryMemoryEntriesActivity(t *testing.T) {
	now := time.Now().UTC()
	past := now.Add(-10 * time.Minute)

	t.Run("returns entries filtered by Since", func(t *testing.T) {
		fake := &fakeMemoryService{
			searchResp: &mpv1.SearchMemoryResponse{
				Entries: []*mpv1.MemoryEntry{
					{
						MemoryId:  "m1",
						Content:   "recent memory",
						ThreadId:  "thread-1",
						UpdatedAt: timestamppb.New(now),
					},
					{
						MemoryId:  "m2",
						Content:   "old memory",
						ThreadId:  "thread-2",
						UpdatedAt: timestamppb.New(past.Add(-30 * time.Minute)), // before Since
					},
				},
			},
		}
		conn := newMemoryServiceConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{LettaBridge: conn})

		out, err := a.QueryMemoryEntriesActivity(
			context.Background(),
			activities.MemoryQueryInput{
				OrgID:    "org-1",
				Since:    past,
				MaxItems: 10,
			},
		)
		require.NoError(t, err)
		require.Len(t, out, 1)
		assert.Equal(t, "m1", out[0].ID)
		assert.Equal(t, "recent memory", out[0].Content)
	})

	t.Run("empty response returns empty slice", func(t *testing.T) {
		fake := &fakeMemoryService{
			searchResp: &mpv1.SearchMemoryResponse{Entries: nil},
		}
		conn := newMemoryServiceConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{LettaBridge: conn})

		out, err := a.QueryMemoryEntriesActivity(
			context.Background(),
			activities.MemoryQueryInput{OrgID: "org-empty", Since: past, MaxItems: 5},
		)
		require.NoError(t, err)
		assert.Empty(t, out)
	})

	t.Run("nil conn returns error", func(t *testing.T) {
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{LettaBridge: nil})
		_, err := a.QueryMemoryEntriesActivity(
			context.Background(),
			activities.MemoryQueryInput{OrgID: "org-1", Since: past, MaxItems: 5},
		)
		require.Error(t, err)
		assert.Equal(t, codes.Unavailable, status.Code(err))
	})
}

// ─── WriteConsolidatedMemoryActivity ─────────────────────────────────────────

func TestWriteConsolidatedMemoryActivity(t *testing.T) {
	entries := []activities.MemoryEntry{
		{ID: "e1", OrgID: "org-1", Content: "fact one", ThreadID: "thread-1", CreatedAt: time.Now()},
		{ID: "e2", OrgID: "org-1", Content: "fact two", ThreadID: "thread-2", CreatedAt: time.Now()},
	}

	t.Run("success", func(t *testing.T) {
		fake := &fakeMemoryService{indexErr: nil}
		conn := newMemoryServiceConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{LettaBridge: conn})

		err := a.WriteConsolidatedMemoryActivity(
			context.Background(),
			activities.WriteMemoryInput{Entries: entries},
		)
		require.NoError(t, err)
	})

	t.Run("gRPC error on IndexMemory is propagated", func(t *testing.T) {
		fake := &fakeMemoryService{
			indexErr: status.Error(codes.Internal, "store failure"),
		}
		conn := newMemoryServiceConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{LettaBridge: conn})

		err := a.WriteConsolidatedMemoryActivity(
			context.Background(),
			activities.WriteMemoryInput{Entries: entries},
		)
		require.Error(t, err)
		assert.Equal(t, codes.Internal, status.Code(err))
	})

	t.Run("nil conn returns error", func(t *testing.T) {
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{LettaBridge: nil})
		err := a.WriteConsolidatedMemoryActivity(
			context.Background(),
			activities.WriteMemoryInput{Entries: entries},
		)
		require.Error(t, err)
		assert.Equal(t, codes.Unavailable, status.Code(err))
	})
}

// ─── SummarizeMemoryActivity ──────────────────────────────────────────────────

func TestSummarizeMemoryActivity(t *testing.T) {
	// Pure local logic — no gRPC connection required.
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{})

	// ThreadID is now REQUIRED for an entry to be consolidatable: an entry that
	// cannot be attributed to a conversation is dropped rather than merged into
	// a shared bucket with other users' memories. This fixture previously omitted
	// it and passed only because of that bucket.
	entries := []activities.MemoryEntry{
		{ID: "e1", ThreadID: "thread-1", Content: "first fact", CreatedAt: time.Now()},
		{ID: "e2", ThreadID: "thread-1", Content: "second fact", CreatedAt: time.Now()},
	}

	t.Run("returns consolidated output", func(t *testing.T) {
		out, err := a.SummarizeMemoryActivity(
			context.Background(),
			activities.ConsolidationInput{Entries: entries},
		)
		require.NoError(t, err)
		assert.NotEmpty(t, out.Summary)
		assert.NotEmpty(t, out.ConsolidatedEntries)
	})

	t.Run("an entry with no thread is not consolidatable", func(t *testing.T) {
		out, err := a.SummarizeMemoryActivity(
			context.Background(),
			activities.ConsolidationInput{
				Entries: []activities.MemoryEntry{{ID: "orphan", Content: "unattributable", CreatedAt: time.Now()}},
			},
		)
		require.NoError(t, err)
		assert.Empty(t, out.ConsolidatedEntries, "an unattributable memory must not be summarised into a shared row")
	})

	t.Run("empty entries returns empty output", func(t *testing.T) {
		out, err := a.SummarizeMemoryActivity(
			context.Background(),
			activities.ConsolidationInput{Entries: nil},
		)
		require.NoError(t, err)
		assert.Empty(t, out.ConsolidatedEntries)
	})
}

// ─── StartScheduledRunActivity ───────────────────────────────────────────────

func TestStartScheduledRunActivity_ExecutesOnlyWithPreparedExecutionIntent(t *testing.T) {
	authorizer := &fakeScheduledRunExecutionAuthorizer{token: "execution-token"}
	fakeSession := &fakeSessionCore{startScheduledRunResp: &mpv1.StartRunResponse{
		RunId:   "task-scheduled-1",
		OwnerId: activities.SystemActorID,
	}}
	conn := newSessionCoreConn(t, fakeSession)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{SessionCore: conn})
	a.SetScheduledRunExecutionAuthorizer(authorizer)

	meta, err := a.StartScheduledRunActivity(
		context.Background(),
		"task-scheduled-1",
		"thread-scheduled-1",
		"org-1",
		"space-1",
		"user-1",
		"schedule-1",
		"2026-08-14T00:00:00Z",
		"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"schedule-1:2026-08-14T00:00:00Z",
		"Summarise the queue",
		"execute",
	)
	require.NoError(t, err)
	require.NotNil(t, fakeSession.startScheduledRunReq)
	assert.Equal(t, 1, authorizer.calls)
	assert.Equal(t, "task-scheduled-1", fakeSession.startScheduledRunReq.GetRunId())
	assert.Equal(t, "thread-scheduled-1", fakeSession.startScheduledRunReq.GetThreadId())
	assert.Equal(t, "space-1", authorizer.intent.SpaceRef)
	assert.Equal(t, "execution-token", fakeSession.startScheduledRunReq.GetControlExecutionDecisionToken())
	assert.Equal(t, "task-scheduled-1", meta.RunID)
	assert.Equal(t, "thread-scheduled-1", meta.ThreadID)
	assert.Equal(t, "org-1", meta.OrgID)
}

func TestExecuteScheduledStepActivityForwardsExactIntentAndDecision(t *testing.T) {
	fakeExecution := &fakeExecutionCore{
		executeScheduledStepResp: &mpv1.ExecuteScheduledStepResponse{
			Status:    "completed",
			ReceiptId: "receipt-step-1",
			Output:    "scheduled result",
		},
	}
	authorizer := &fakeScheduledStepDecisionAuthorizer{token: "control-step-decision"}
	conn := newExecutionCoreConn(t, fakeExecution)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{ExecutionCore: conn})
	a.SetScheduledStepDecisionAuthorizer(authorizer)
	intent := activities.ScheduledStepExecutionIntent{
		OrgID:          "org-1",
		SpaceRef:       "space-1",
		SubjectID:      "user-1",
		RunID:          "run-1",
		ThreadID:       "thread-1",
		ScheduleID:     "schedule-1",
		FireKey:        "fire-1",
		TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		StepID:         "run-1:step:0",
		StepIndex:      0,
		PolicyDigest:   "sha256:" + strings.Repeat("b", 64),
		IdempotencyKey: "fire-1:step:0",
	}

	step, err := a.ExecuteScheduledStepActivity(context.Background(), intent)
	require.NoError(t, err)
	require.NotNil(t, fakeExecution.executeScheduledStepReq)
	assert.Equal(t, 1, authorizer.calls)
	assert.Equal(t, intent, authorizer.intent)
	assert.Equal(t, "control-step-decision", fakeExecution.executeScheduledStepReq.GetControlDecisionToken())
	assert.Equal(t, "run-1", fakeExecution.executeScheduledStepReq.GetRunId())
	assert.Equal(t, "space-1", fakeExecution.executeScheduledStepReq.GetSpaceId())
	assert.Equal(t, "run-1:step:0", fakeExecution.executeScheduledStepReq.GetStepId())
	assert.Equal(t, "fire-1:step:0", fakeExecution.executeScheduledStepReq.GetIdempotencyKey())
	assert.Equal(t, "scheduled-step", step.ToolName)
	assert.Equal(t, "scheduled result", step.Output)
	assert.True(t, step.Completed)
	assert.Equal(t, "receipt-step-1", step.Metadata["receipt_id"])
}

func TestExecuteScheduledStepActivityStopsOnUnknownOutcome(t *testing.T) {
	fakeExecution := &fakeExecutionCore{
		executeScheduledStepResp: &mpv1.ExecuteScheduledStepResponse{
			Status:         "unknown_outcome",
			ReceiptId:      "receipt-unknown-1",
			UnknownOutcome: true,
		},
	}
	authorizer := &fakeScheduledStepDecisionAuthorizer{token: "control-step-decision"}
	conn := newExecutionCoreConn(t, fakeExecution)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{ExecutionCore: conn})
	a.SetScheduledStepDecisionAuthorizer(authorizer)

	intent := activities.ScheduledStepExecutionIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", RunID: "run-unknown", ThreadID: "thread-unknown",
		ScheduleID: "schedule-1", FireKey: "fire-1", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		StepID: "run-unknown:step:0", StepIndex: 0, PolicyDigest: "sha256:" + strings.Repeat("b", 64), IdempotencyKey: "fire-1:step:0",
	}

	step, err := a.ExecuteScheduledStepActivity(context.Background(), intent)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "outcome is unknown")
	assert.True(t, step.UnknownOutcome)
	assert.Equal(t, "receipt-unknown-1", step.Metadata["receipt_id"])
	assert.Equal(t, "true", step.Metadata["unknown_outcome"])
}

func TestExecuteScheduledStepActivityStopsOnFailedOutcome(t *testing.T) {
	fakeExecution := &fakeExecutionCore{
		executeScheduledStepResp: &mpv1.ExecuteScheduledStepResponse{
			Status:    "failed",
			ReceiptId: "receipt-failed-1",
			Error:     "run_cancelled",
		},
	}
	authorizer := &fakeScheduledStepDecisionAuthorizer{token: "control-step-decision"}
	conn := newExecutionCoreConn(t, fakeExecution)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{ExecutionCore: conn})
	a.SetScheduledStepDecisionAuthorizer(authorizer)

	intent := activities.ScheduledStepExecutionIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", RunID: "run-failed", ThreadID: "thread-failed",
		ScheduleID: "schedule-1", FireKey: "fire-1", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		StepID: "run-failed:step:0", StepIndex: 0, PolicyDigest: "sha256:" + strings.Repeat("b", 64), IdempotencyKey: "fire-1:step:0",
	}

	step, err := a.ExecuteScheduledStepActivity(context.Background(), intent)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "must not advance")
	assert.Equal(t, 0, step.StepIndex)
	assert.Equal(t, "receipt-failed-1", step.Metadata["receipt_id"])
	assert.Equal(t, "run_cancelled", step.Metadata["error_code"])
}

func TestExecuteScheduledStepActivityFailsClosedOnInvalidResponse(t *testing.T) {
	baseIntent := activities.ScheduledStepExecutionIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", RunID: "run-invalid", ThreadID: "thread-invalid",
		ScheduleID: "schedule-1", FireKey: "fire-1", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		StepID: "run-invalid:step:0", StepIndex: 0, PolicyDigest: "sha256:" + strings.Repeat("b", 64), IdempotencyKey: "fire-1:step:0",
	}

	for name, response := range map[string]*mpv1.ExecuteScheduledStepResponse{
		"nil response":   nil,
		"unknown status": {Status: "pending", ReceiptId: "receipt-invalid-1"},
	} {
		t.Run(name, func(t *testing.T) {
			fakeExecution := &fakeExecutionCore{executeScheduledStepResp: response}
			a := activities.NewActivities(testLogger(), &grpcclient.Clients{ExecutionCore: newExecutionCoreConn(t, fakeExecution)})
			a.SetScheduledStepDecisionAuthorizer(&fakeScheduledStepDecisionAuthorizer{token: "control-step-decision"})

			step, err := a.ExecuteScheduledStepActivity(context.Background(), baseIntent)
			require.Error(t, err)
			assert.Contains(t, err.Error(), "must not advance")
			if response != nil {
				assert.Equal(t, "invalid_status", step.Metadata["error_code"])
				assert.Equal(t, "pending", step.Metadata["status"])
			}
		})
	}
}

func TestStartScheduledRunActivity_FailsClosedWithoutSessionCoreOrExecutionAuthority(t *testing.T) {
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{SessionCore: nil})
	_, err := a.StartScheduledRunActivity(context.Background(), "task-1", "thread-1", "org-1", "space-1", "user-1", "schedule-1", "fire", "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "id", "g", "execute")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "session-core unavailable for prepared scheduled run")

	a = activities.NewActivities(testLogger(), &grpcclient.Clients{SessionCore: nil})
	fake := &fakeSessionCore{startScheduledRunResp: &mpv1.StartRunResponse{RunId: "task-1", OwnerId: activities.SystemActorID}}
	a = activities.NewActivities(testLogger(), &grpcclient.Clients{SessionCore: newSessionCoreConn(t, fake)})
	_, err = a.StartScheduledRunActivity(context.Background(), "task-1", "thread-1", "org-1", "space-1", "user-1", "schedule-1", "fire", "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "id", "g", "execute")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "Control scheduled-run execution authority is not configured")
}

func TestStartScheduledRunActivity_RejectsSessionCoreReceiptMismatch(t *testing.T) {
	authorizer := &fakeScheduledRunExecutionAuthorizer{token: "execution-token"}
	fakeSession := &fakeSessionCore{startScheduledRunResp: &mpv1.StartRunResponse{
		RunId:   "task-other",
		OwnerId: activities.SystemActorID,
	}}
	conn := newSessionCoreConn(t, fakeSession)
	a := activities.NewActivities(testLogger(), &grpcclient.Clients{SessionCore: conn})
	a.SetScheduledRunExecutionAuthorizer(authorizer)

	_, err := a.StartScheduledRunActivity(
		context.Background(),
		"task-scheduled-1",
		"thread-scheduled-1",
		"org-1",
		"space-1",
		"user-1",
		"schedule-1",
		"2026-08-14T00:00:00Z",
		"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"schedule-1:2026-08-14T00:00:00Z",
		"Summarise the queue",
		"execute",
	)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "prepared scheduled run task-scheduled-1 received mismatched Session Core receipt")

	fakeSession.startScheduledRunResp = &mpv1.StartRunResponse{RunId: "task-scheduled-1", OwnerId: "someone-else"}
	_, err = a.StartScheduledRunActivity(
		context.Background(),
		"task-scheduled-1",
		"thread-scheduled-1",
		"org-1",
		"space-1",
		"user-1",
		"schedule-1",
		"2026-08-14T00:00:00Z",
		"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"schedule-1:2026-08-14T00:00:00Z",
		"Summarise the queue",
		"execute",
	)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "prepared scheduled run task-scheduled-1 received mismatched Session Core receipt")
}
