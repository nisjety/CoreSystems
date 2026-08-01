package activities_test

import (
	"context"
	"log/slog"
	"net"
	"os"
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

	promoteErr error
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

func (f *fakeCapabilityCore) PromoteSkill(
	_ context.Context,
	_ *mpv1.PromoteSkillRequest,
) (*mpv1.PromoteSkillResponse, error) {
	return &mpv1.PromoteSkillResponse{}, f.promoteErr
}

type fakeMemoryService struct {
	mpv1.UnimplementedMemoryServiceServer

	searchResp *mpv1.SearchMemoryResponse
	searchErr  error

	indexErr error
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

// ─── UpdateRegistryActivity ───────────────────────────────────────────────────

func TestUpdateRegistryActivity(t *testing.T) {
	t.Run("success", func(t *testing.T) {
		fake := &fakeCapabilityCore{promoteErr: nil}
		conn := newCapabilityCoreConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{CapabilityCore: conn})

		err := a.UpdateRegistryActivity(
			context.Background(),
			activities.RegistryUpdateInput{SkillID: "s1", FromScope: "dev", NewScope: "staging"},
		)
		require.NoError(t, err)
	})

	t.Run("gRPC error is propagated", func(t *testing.T) {
		fake := &fakeCapabilityCore{
			promoteErr: status.Error(codes.NotFound, "skill not found"),
		}
		conn := newCapabilityCoreConn(t, fake)
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{CapabilityCore: conn})

		err := a.UpdateRegistryActivity(
			context.Background(),
			activities.RegistryUpdateInput{SkillID: "missing", FromScope: "dev", NewScope: "staging"},
		)
		require.Error(t, err)
		assert.Equal(t, codes.NotFound, status.Code(err))
	})

	t.Run("nil conn returns error", func(t *testing.T) {
		a := activities.NewActivities(testLogger(), &grpcclient.Clients{CapabilityCore: nil})
		err := a.UpdateRegistryActivity(
			context.Background(),
			activities.RegistryUpdateInput{SkillID: "s1", FromScope: "dev", NewScope: "staging"},
		)
		require.Error(t, err)
		assert.Equal(t, codes.Unavailable, status.Code(err))
	})
}

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
