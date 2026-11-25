package server

import (
	"context"
	"strings"
	"sync"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/sitepolicy"
	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/metric/noop"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// outcomeRecorder stands in for telemetry.SitePolicyVerdictsTotal and keeps the outcome label of every verdict it counts.
type outcomeRecorder struct {
	noop.Int64Counter
	mu       sync.Mutex
	outcomes []string
}

func (o *outcomeRecorder) Add(_ context.Context, _ int64, options ...metric.AddOption) {
	attrs := metric.NewAddConfig(options).Attributes()
	outcome, _ := attrs.Value(attribute.Key("outcome"))
	o.mu.Lock()
	defer o.mu.Unlock()
	o.outcomes = append(o.outcomes, outcome.AsString())
}

// recordVerdicts swaps the verdict counter for a recorder until the test ends.
func recordVerdicts(t *testing.T) *outcomeRecorder {
	t.Helper()
	rec := &outcomeRecorder{}
	previous := telemetry.SitePolicyVerdictsTotal
	telemetry.SitePolicyVerdictsTotal = rec
	t.Cleanup(func() { telemetry.SitePolicyVerdictsTotal = previous })
	return rec
}

// spaceFloorServer is a server with the Space floor switch, the site policy mode and a resolver set.
func spaceFloorServer(store grant.Repository, principal authctx.Principal, mode sitepolicy.Mode, floor bool, sites sitepolicy.Resolver) *Server {
	return testServerFor(store, principal).WithOptions(Options{SitePolicy: mode, SpaceFloor: floor, SiteResolver: sites})
}

// spaceRequest is an ordinary grant for one host in a Space.
func spaceRequest(spaceID string) *AcquireGrantRequest {
	return &AcquireGrantRequest{OrgId: "org1", SessionKey: "session1", Mode: "cloud", AllowedDomains: []string{"example.com"}, SpaceId: spaceID}
}

// F-f03-03: with BROWSER_SPACE_FLOOR_ENABLED off, a Space grant is refused before the site policy is read and before any
// grant is written, and the refusal is counted by reason and by verdict.
func TestSpaceFloorSwitchOffRefusesBeforeThePolicyIsRead(t *testing.T) {
	denials := recordDenials(t)
	verdicts := recordVerdicts(t)
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	sites := &fakeSites{effective: map[string]string{"example.com": sitepolicy.EffectiveAct}}
	srv := spaceFloorServer(store, userPrincipal(false), sitepolicy.ModeEnforce, false, sites)

	resp, err := srv.AcquireGrant(context.Background(), spaceRequest("space-1"))
	if resp != nil || status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("got (%v, %v), want a FailedPrecondition refusal", resp, err)
	}
	if reason := errorInfoReason(t, err); reason != reasonSpaceFloorUnavailable {
		t.Fatalf("ErrorInfo reason = %q, want %q", reason, reasonSpaceFloorUnavailable)
	}
	if sites.calls != 0 {
		t.Fatalf("org-core was read %d times for a refused Space grant, want none", sites.calls)
	}
	if store.writes != 0 {
		t.Fatalf("a refused Space grant wrote %d session grants, want none", store.writes)
	}
	if len(denials.reasons) != 1 || denials.reasons[0] != "space_floor_unavailable" {
		t.Fatalf("denial counter reasons = %v, want one space_floor_unavailable", denials.reasons)
	}
	if len(verdicts.outcomes) != 1 || verdicts.outcomes[0] != "space_floor" {
		t.Fatalf("verdict outcomes = %v, want one space_floor", verdicts.outcomes)
	}
}

// F-f03-03: the switch on with the site policy off still refuses a Space grant, because the floor is read through the
// policy and no policy is read here.
func TestSpaceFloorSwitchOnWithThePolicyOffStillRefuses(t *testing.T) {
	denials := recordDenials(t)
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	sites := &fakeSites{}
	for _, mode := range []sitepolicy.Mode{sitepolicy.ModeOff, ""} {
		srv := spaceFloorServer(store, userPrincipal(false), mode, true, sites)
		if _, err := srv.AcquireGrant(context.Background(), spaceRequest("space-1")); status.Code(err) != codes.FailedPrecondition {
			t.Fatalf("site policy %q: code = %v, want FailedPrecondition", mode, status.Code(err))
		}
	}
	if sites.calls != 0 || store.writes != 0 {
		t.Fatalf("org-core calls = %d, writes = %d; want none for a refused Space grant", sites.calls, store.writes)
	}
	if len(denials.reasons) != 2 {
		t.Fatalf("denial counter reasons = %v, want two space_floor_unavailable", denials.reasons)
	}
}

// F-f03-03: with the switch on and the policy enforced, the Space is sent with the resolve, and the grant is issued
// with the Space's answer.
func TestSpaceFloorSwitchOnResolvesWithTheSpaceAndIssues(t *testing.T) {
	denials := recordDenials(t)
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	sites := &fakeSites{effective: map[string]string{"example.com": sitepolicy.EffectiveAct}}
	srv := spaceFloorServer(store, userPrincipal(false), sitepolicy.ModeEnforce, true, sites)

	if _, err := srv.AcquireGrant(context.Background(), spaceRequest("space-1")); err != nil {
		t.Fatalf("a Space grant with the floor on was refused: %v", err)
	}
	if sites.lastSpace != "space-1" {
		t.Fatalf("the resolve named Space %q, want space-1", sites.lastSpace)
	}
	if store.writes != 1 {
		t.Fatalf("session grants written = %d, want 1", store.writes)
	}
	if len(denials.reasons) != 0 {
		t.Fatalf("an allowed Space grant counted as denied: %v", denials.reasons)
	}
}

// F-f03-03: org-core's all-deny for the Space is a site_policy denial, the same refusal an org-wide deny gets.
func TestSpaceFloorAllDenyIsASitePolicyDenial(t *testing.T) {
	denials := recordDenials(t)
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	sites := &fakeSites{effective: map[string]string{"example.com": sitepolicy.EffectiveDeny}}
	srv := spaceFloorServer(store, userPrincipal(false), sitepolicy.ModeEnforce, true, sites)

	resp, err := srv.AcquireGrant(context.Background(), spaceRequest("space-1"))
	if resp != nil || status.Code(err) != codes.PermissionDenied {
		t.Fatalf("got (%v, %v), want a PermissionDenied refusal", resp, err)
	}
	if reason := errorInfoReason(t, err); reason != siteDeniedReason {
		t.Fatalf("ErrorInfo reason = %q, want %q", reason, siteDeniedReason)
	}
	if sites.lastSpace != "space-1" {
		t.Fatalf("the resolve named Space %q, want space-1", sites.lastSpace)
	}
	if store.writes != 0 {
		t.Fatalf("a denied Space grant wrote %d session grants, want none", store.writes)
	}
	if len(denials.reasons) != 1 || denials.reasons[0] != "site_policy" {
		t.Fatalf("denial counter reasons = %v, want one site_policy", denials.reasons)
	}
}

// F-f03-03: a grant with no Space reads the org-wide answer exactly as before, whatever the switch says.
func TestUnscopedGrantIsUnchangedByTheSpaceFloor(t *testing.T) {
	for _, floor := range []bool{false, true} {
		denials := recordDenials(t)
		store := &writeRecorder{Repository: grant.NewMemoryRepository()}
		sites := &fakeSites{effective: map[string]string{"example.com": sitepolicy.EffectiveAct}}
		srv := spaceFloorServer(store, userPrincipal(false), sitepolicy.ModeEnforce, floor, sites)

		if _, err := srv.AcquireGrant(context.Background(), spaceRequest("")); err != nil {
			t.Fatalf("floor %v: an unscoped grant was refused: %v", floor, err)
		}
		if sites.lastSpace != "" {
			t.Fatalf("floor %v: the org-wide resolve named Space %q", floor, sites.lastSpace)
		}
		if store.writes != 1 || len(denials.reasons) != 0 {
			t.Fatalf("floor %v: writes = %d, denials = %v; want one write and no denial", floor, store.writes, denials.reasons)
		}
	}
}

// F-f03-03: a blank or padded Space id is refused even with the switch on, and the policy is never read for it.
func TestSpaceFloorRefusesANonCanonicalSpaceID(t *testing.T) {
	denials := recordDenials(t)
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	sites := &fakeSites{}
	srv := spaceFloorServer(store, userPrincipal(false), sitepolicy.ModeEnforce, true, sites)

	for _, spaceID := range []string{"  ", " space-1", "space-1 "} {
		if _, err := srv.AcquireGrant(context.Background(), spaceRequest(spaceID)); status.Code(err) != codes.FailedPrecondition {
			t.Fatalf("space_id %q: code = %v, want FailedPrecondition", spaceID, status.Code(err))
		}
	}
	if sites.calls != 0 || store.writes != 0 || len(denials.reasons) != 3 {
		t.Fatalf("calls = %d, writes = %d, denials = %v; want refusals only", sites.calls, store.writes, denials.reasons)
	}
}

// F-f03-03: a Space on a path that reads no site policy with the Space is refused with the switch on: a sensitive
// approval, and a human-browse grant.
func TestSpaceFloorRefusesPathsThatReadNoSitePolicy(t *testing.T) {
	denials := recordDenials(t)
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	sites := &fakeSites{}
	srv := spaceFloorServer(store, userPrincipal(false), sitepolicy.ModeEnforce, true, sites)

	sensitive := spaceRequest("space-1")
	sensitive.AllowedActions = []string{"click"}
	sensitive.ParentGrantId = "parent-1"
	humanBrowse := spaceRequest("space-1")
	humanBrowse.GrantKind = grantKindHumanBrowse
	for name, req := range map[string]*AcquireGrantRequest{"sensitive approval": sensitive, "human browse": humanBrowse} {
		if _, err := srv.AcquireGrant(context.Background(), req); status.Code(err) != codes.FailedPrecondition {
			t.Fatalf("%s: code = %v, want FailedPrecondition", name, status.Code(err))
		}
	}
	if sites.calls != 0 || store.writes != 0 || len(denials.reasons) != 2 {
		t.Fatalf("calls = %d, writes = %d, denials = %v; want refusals only", sites.calls, store.writes, denials.reasons)
	}
}

// F-f03-03: the Space floor serves an assistant task grant too, which resolves the Space on its own check.
func TestSpaceFloorAssistantTaskResolvesWithTheSpace(t *testing.T) {
	var named []string
	resolver := assistantTaskSites(func(_ context.Context, _, _, space string, hosts []string) (sitepolicy.Resolution, error) {
		named = append(named, space)
		return sitepolicy.Resolution{Decisions: []sitepolicy.Decision{{Host: hosts[0], Effective: sitepolicy.EffectiveAct}}}, nil
	})
	store := &writeRecorder{Repository: grant.NewMemoryRepository()}
	srv := testServerFor(store, userPrincipal(false)).WithOptions(Options{
		AssistantTaskGrantsEnabled: true, SitePolicy: sitepolicy.ModeEnforce, SpaceFloor: true, SiteResolver: resolver,
	})
	req := spaceRequest("space-1")
	req.GrantKind = grantKindAssistantTask
	if _, err := srv.AcquireGrant(context.Background(), req); err != nil {
		t.Fatalf("assistant task Space grant: %v", err)
	}
	if len(named) != 1 || named[0] != "space-1" {
		t.Fatalf("assistant task resolve named Spaces %q, want [space-1]", named)
	}
}

// F-f03-03: an api key grant sends its Space with the resolve, like an ordinary grant does.
func TestSpaceFloorAPIKeyGrantResolvesWithTheSpace(t *testing.T) {
	store := grant.NewMemoryRepository()
	sites := &fakeSites{effective: map[string]string{"example.com": sitepolicy.EffectiveAct}}
	srv := keyServer(store, keyCaller("key-1"), true).WithOptions(Options{
		SitePolicy: sitepolicy.ModeEnforce, SpaceFloor: true, SiteResolver: sites,
	})
	req := keyAcquireReq()
	req.SpaceId = "space-1"
	if _, err := srv.AcquireGrant(context.Background(), req); err != nil {
		t.Fatalf("api key Space grant: %v", err)
	}
	if sites.lastSpace != "space-1" {
		t.Fatalf("the api key resolve named Space %q, want space-1", sites.lastSpace)
	}
}

// F-f03-03: which mint paths the Space floor serves. Only the paths that read the site policy with the Space, with the
// switch on, the policy read on, a canonical id and no sensitive scope.
func TestSpaceFloorServesOnlyThePolicyReadingPaths(t *testing.T) {
	user := userPrincipal(false)
	key := keyCaller("key-1")
	if key.PrincipalType != authz.PrincipalAPIKey {
		t.Fatalf("keyCaller principal type = %q, want %q", key.PrincipalType, authz.PrincipalAPIKey)
	}
	cases := []struct {
		name      string
		floor     bool
		mode      sitepolicy.Mode
		principal authctx.Principal
		edit      func(*AcquireGrantRequest)
		want      bool
	}{
		{name: "ordinary", floor: true, mode: sitepolicy.ModeEnforce, principal: user, want: true},
		{name: "shadow ordinary", floor: true, mode: sitepolicy.ModeShadow, principal: user, want: true},
		{name: "assistant task", floor: true, mode: sitepolicy.ModeEnforce, principal: user,
			edit: func(r *AcquireGrantRequest) { r.GrantKind = grantKindAssistantTask }, want: true},
		{name: "origin extension", floor: true, mode: sitepolicy.ModeEnforce, principal: user,
			edit: func(r *AcquireGrantRequest) { r.GrantKind = grantKindOriginExtension }, want: true},
		{name: "api key", floor: true, mode: sitepolicy.ModeEnforce, principal: key, want: true},
		{name: "human browse", floor: true, mode: sitepolicy.ModeEnforce, principal: user,
			edit: func(r *AcquireGrantRequest) { r.GrantKind = grantKindHumanBrowse }, want: false},
		{name: "sensitive approval", floor: true, mode: sitepolicy.ModeEnforce, principal: user,
			edit: func(r *AcquireGrantRequest) { r.AllowedActions = []string{"click"} }, want: false},
		{name: "switch off", floor: false, mode: sitepolicy.ModeEnforce, principal: user, want: false},
		{name: "policy off", floor: true, mode: sitepolicy.ModeOff, principal: user, want: false},
		{name: "policy unset", floor: true, mode: "", principal: user, want: false},
		{name: "padded id", floor: true, mode: sitepolicy.ModeEnforce, principal: user,
			edit: func(r *AcquireGrantRequest) { r.SpaceId = " space-1" }, want: false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			srv := spaceFloorServer(grant.NewMemoryRepository(), tc.principal, tc.mode, tc.floor, nil)
			req := spaceRequest("space-1")
			if tc.edit != nil {
				tc.edit(req)
			}
			if got := srv.spaceFloorServes(tc.principal, req); got != tc.want {
				t.Fatalf("spaceFloorServes = %v, want %v (kind %q, actions %v, space %q)",
					got, tc.want, req.GetGrantKind(), req.GetAllowedActions(), req.GetSpaceId())
			}
		})
	}
}

// The Space floor's closed reasons carry no Space id: the refusal text and its reason are the same for every Space.
func TestSpaceFloorRefusalNamesNoSpace(t *testing.T) {
	store := grant.NewMemoryRepository()
	srv := spaceFloorServer(store, userPrincipal(false), sitepolicy.ModeEnforce, false, &fakeSites{})
	_, err := srv.AcquireGrant(context.Background(), spaceRequest("space-secret-7"))
	if err == nil || strings.Contains(err.Error(), "space-secret-7") || strings.Contains(errorInfoReason(t, err), "space-secret-7") {
		t.Fatalf("the refusal names the Space: %v", err)
	}
}
