package server

import (
	"context"
	"sync"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/metric/noop"
	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// denialRecorder stands in for telemetry.GrantsDeniedTotal and keeps the reason
// label of every refusal it counts. The production counter carries nothing else.
type denialRecorder struct {
	noop.Int64Counter
	mu      sync.Mutex
	reasons []string
}

func (d *denialRecorder) Add(_ context.Context, _ int64, options ...metric.AddOption) {
	attrs := metric.NewAddConfig(options).Attributes()
	reason, _ := attrs.Value(attribute.Key("reason"))
	d.mu.Lock()
	defer d.mu.Unlock()
	d.reasons = append(d.reasons, reason.AsString())
}

// recordDenials swaps the denial counter for a recorder until the test ends. The
// tests that use it do not run in parallel, so nothing reads the counter meanwhile.
func recordDenials(t *testing.T) *denialRecorder {
	t.Helper()
	rec := &denialRecorder{}
	previous := telemetry.GrantsDeniedTotal
	telemetry.GrantsDeniedTotal = rec
	t.Cleanup(func() { telemetry.GrantsDeniedTotal = previous })
	return rec
}

// writeRecorder counts the session grants the broker writes, so a refusal that
// happens after a write would be caught.
type writeRecorder struct {
	grant.Repository
	writes int
}

func (w *writeRecorder) CreateSession(ctx context.Context, p grant.CreateSessionParams) (*grant.Grant, error) {
	w.writes++
	return w.Repository.CreateSession(ctx, p)
}

// errorInfoReason returns the ErrorInfo reason a refusal carries, or "".
func errorInfoReason(t *testing.T, err error) string {
	t.Helper()
	st, ok := status.FromError(err)
	if !ok {
		t.Fatalf("refusal is not a gRPC status: %v", err)
	}
	for _, detail := range st.Details() {
		if info, ok := detail.(*errdetails.ErrorInfo); ok {
			return info.GetReason()
		}
	}
	return ""
}

// F-f02-19: a non-empty space_id is refused until the Space floor is wired. The
// refusal happens before any grant is written and is counted by reason only.
func TestSpaceGrantIsRefusedBeforeAnyGrantIsWritten(t *testing.T) {
	rec := recordDenials(t)
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	srv := testServerFor(store, authctx.Principal{OrganizationID: "org1", ActorID: "user1", PrincipalType: "user"})

	// The whitespace-only value is refused too: the raw value is tested.
	for _, spaceID := range []string{"space-1", "  "} {
		resp, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
			OrgId: "org1", SessionKey: "session1", Mode: "cloud", AllowedDomains: []string{"example.com"}, SpaceId: spaceID,
		})
		if resp != nil || status.Code(err) != codes.FailedPrecondition {
			t.Fatalf("space_id %q: got (%v, %v), want a FailedPrecondition refusal", spaceID, resp, err)
		}
		if reason := errorInfoReason(t, err); reason != reasonSpaceFloorUnavailable {
			t.Fatalf("space_id %q: ErrorInfo reason = %q, want %q", spaceID, reason, reasonSpaceFloorUnavailable)
		}
	}
	if store.writes != 0 {
		t.Fatalf("refused Space grants wrote %d session grants, want none", store.writes)
	}
	if want := []string{"space_floor_unavailable", "space_floor_unavailable"}; len(rec.reasons) != len(want) || rec.reasons[0] != want[0] || rec.reasons[1] != want[1] {
		t.Fatalf("denial counter reasons = %v, want %v", rec.reasons, want)
	}
}

// The API-key path has its own gate (key grants off by default). The Space refusal
// comes first, so an API key asking for a Space gets the Space reason, not the gate's.
func TestSpaceGrantIsRefusedForAnAPIKeyBeforeTheKeyGate(t *testing.T) {
	rec := recordDenials(t)
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	srv := testServerFor(store, authctx.Principal{OrganizationID: "org1", ActorID: "user1", PrincipalType: authz.PrincipalAPIKey})

	resp, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "session1", Mode: "cloud", AllowedDomains: []string{"example.com"}, SpaceId: "space-1",
	})
	if resp != nil || status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("API-key Space grant: got (%v, %v), want a FailedPrecondition refusal", resp, err)
	}
	if reason := errorInfoReason(t, err); reason != reasonSpaceFloorUnavailable {
		t.Fatalf("API-key Space grant: ErrorInfo reason = %q, want %q", reason, reasonSpaceFloorUnavailable)
	}
	if store.writes != 0 {
		t.Fatalf("refused API-key Space grant wrote %d session grants, want none", store.writes)
	}
	if len(rec.reasons) != 1 || rec.reasons[0] != "space_floor_unavailable" {
		t.Fatalf("denial counter reasons = %v, want one space_floor_unavailable", rec.reasons)
	}
}

// An empty space_id is the ordinary path, unchanged: the same request that the
// grant tests issue succeeds and counts no denial.
func TestAnEmptySpaceIDLeavesTheOrdinaryGrantUnchanged(t *testing.T) {
	rec := recordDenials(t)
	srv := testServerFor(grant.NewMemoryRepository(), authctx.Principal{OrganizationID: "org1", ActorID: "user1", PrincipalType: "user"})

	if _, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "session1", Mode: "cloud", AllowedDomains: []string{"example.com"},
	}); err != nil {
		t.Fatalf("an ordinary grant with no space_id was refused: %v", err)
	}
	if len(rec.reasons) != 0 {
		t.Fatalf("an ordinary grant counted as denied: %v", rec.reasons)
	}
}
