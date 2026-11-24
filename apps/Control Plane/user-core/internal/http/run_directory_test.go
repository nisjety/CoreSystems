package http

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/directory"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// A-REC-03: POST /api/v1/internal/spaces/run-directory/people. Fakes stand in
// for Session Core, the Space repository and Postgres; the real route
// (registered through the Space seam) and the real guard run.

const runDirectoryPath = "/api/v1/internal/spaces/run-directory/people"

type fakeRunDirectoryAuthority struct {
	authority spaces.RunActionAuthority
	err       error
	calls     int
}

func (f *fakeRunDirectoryAuthority) ResolveRunActionAuthority(_ context.Context, _, _ string) (spaces.RunActionAuthority, error) {
	f.calls++
	return f.authority, f.err
}

// scopedPeopleDirectory answers the room-excluded query with `outside`.
type scopedPeopleDirectory struct {
	everyone directory.PeopleResult
	outside  directory.PeopleResult
	err      error
	queries  []directory.PeopleQuery
}

func (f *scopedPeopleDirectory) SearchPeople(_ context.Context, q directory.PeopleQuery) (directory.PeopleResult, error) {
	f.queries = append(f.queries, q)
	if f.err != nil {
		return directory.PeopleResult{}, f.err
	}
	if q.ExcludeMemberOf != "" {
		return f.outside, nil
	}
	return f.everyone, nil
}

func runDirectoryAuthority() spaces.RunActionAuthority {
	return spaces.RunActionAuthority{
		RunID: "run-1", OrgID: "org-bound", SubjectID: "subject-1", ThreadID: "thread-1", SpaceRef: "room-1",
		RecipientAudienceRef: "aud", RecipientAudienceRevision: 1, RecipientAudienceHash: "hash",
		PrivacyPolicyRef: "policy", RunContextAuthorizationRef: "ctx", AuthorityRevision: 1, RunStatus: "running",
	}
}

type runDirectoryFixture struct {
	server    *Server
	authority *fakeRunDirectoryAuthority
	people    *scopedPeopleDirectory
	member    runDirectoryMembership
	memberErr error
	admitted  bool
	memberArg []string
}

func newRunDirectoryFixture(t *testing.T) *runDirectoryFixture {
	t.Helper()
	fx := &runDirectoryFixture{
		authority: &fakeRunDirectoryAuthority{authority: runDirectoryAuthority()},
		people: &scopedPeopleDirectory{
			everyone: directory.PeopleResult{People: []directory.Person{
				{SubjectID: "u-ada", DisplayName: "Ada Lovelace", Email: "ada@example.test"},
				{SubjectID: "u-adam", DisplayName: "Adam Smith", Email: "adam@example.test"},
			}},
			outside: directory.PeopleResult{People: []directory.Person{
				{SubjectID: "u-adam", DisplayName: "Adam Smith", Email: "adam@example.test"},
			}},
		},
		member:   runDirectoryMembership{Role: spaces.RoleViewer},
		admitted: true,
	}
	fx.server = &Server{}
	fx.server.SetRunActionAuthorityResolver(fx.authority)
	fx.server.SetPeopleDirectory(fx.people)
	fx.server.runDirectoryMembership = func(_ context.Context, spaceRef, orgID, subjectID string) (runDirectoryMembership, error) {
		fx.memberArg = []string{spaceRef, orgID, subjectID}
		return fx.member, fx.memberErr
	}
	previous := runDirectoryAdmitted
	runDirectoryAdmitted = func(actionID, role string) bool {
		return fx.admitted && actionID == "space.directory.resolve" && role != ""
	}
	t.Cleanup(func() { runDirectoryAdmitted = previous })
	return fx
}

func executionCoreDirectoryCaller() orgPolicyCaller {
	return orgPolicyCaller{authMethod: "service_principal", serviceID: "execution-core", scopes: []string{runDirectoryReadScope}}
}

func runDirectoryCall(s *Server, caller orgPolicyCaller, body string) *httptest.ResponseRecorder {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	engine.Use(func(c *gin.Context) {
		if caller.omitContext {
			return
		}
		c.Set("auth_method", caller.authMethod)
		c.Set("service_id", caller.serviceID)
		c.Set("delegation_verified", caller.delegated)
		c.Set("service_scopes", caller.scopes)
		c.Set("user_id", caller.userID)
		c.Set("org_id", caller.orgID)
	})
	group := engine.Group("/api/v1/internal/spaces")
	for _, register := range spaceFeatureRouteRegistrars {
		register(s, group)
	}
	w := httptest.NewRecorder()
	engine.ServeHTTP(w, httptest.NewRequest(http.MethodPost, runDirectoryPath, strings.NewReader(body)))
	return w
}

type runDirectoryBody struct {
	Data struct {
		People            []runDirectoryPerson `json:"people"`
		ZeroDataRetention bool                 `json:"zero_data_retention"`
	} `json:"data"`
}

const runDirectoryValidBody = `{"run_id":"run-1","org_id":"org-selector","query":"ada"}`

func TestRunDirectoryRouteRegistersOnceWithItsScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	for _, register := range spaceFeatureRouteRegistrars {
		register(&Server{}, group)
	}
	if got := routeCounts(engine.Routes())["POST "+runDirectoryPath]; got != 1 {
		t.Fatalf("POST %s registered %d times", runDirectoryPath, got)
	}
	if scope, ok := coreSpaceScope(http.MethodPost, runDirectoryPath); !ok || scope != runDirectoryReadScope {
		t.Fatalf("scope = %q, %v", scope, ok)
	}
}

func TestRunDirectoryNeedsExecutionCoreAndTheScope(t *testing.T) {
	cases := []struct {
		name   string
		caller orgPolicyCaller
	}{
		{"gateway", func() orgPolicyCaller { c := executionCoreDirectoryCaller(); c.serviceID = "verevon-gateway"; return c }()},
		{"user", func() orgPolicyCaller { c := executionCoreDirectoryCaller(); c.authMethod = "user"; return c }()},
		{"decision scope is not enough", func() orgPolicyCaller {
			c := executionCoreDirectoryCaller()
			c.scopes = []string{"spaces:agent-action:reauthorize"}
			return c
		}()},
		{"gateway directory scope is not enough", func() orgPolicyCaller {
			c := executionCoreDirectoryCaller()
			c.scopes = []string{"directory:read:self"}
			return c
		}()},
		{"no context", orgPolicyCaller{omitContext: true}},
	}
	for _, tc := range cases {
		fx := newRunDirectoryFixture(t)
		w := runDirectoryCall(fx.server, tc.caller, runDirectoryValidBody)
		if w.Code != http.StatusForbidden || directoryErrorCode(t, w) != "run_directory_reader_required" {
			t.Errorf("%s: %d %s", tc.name, w.Code, w.Body.String())
		}
		if fx.authority.calls != 0 || len(fx.people.queries) != 0 {
			t.Errorf("%s: reached Session Core or the directory", tc.name)
		}
	}
}

func TestRunDirectoryReadsAsTheRunSubjectInTheRunOrgAndMarksTheRoom(t *testing.T) {
	fx := newRunDirectoryFixture(t)
	w := runDirectoryCall(fx.server, executionCoreDirectoryCaller(), runDirectoryValidBody)
	if w.Code != http.StatusOK {
		t.Fatalf("want 200, got %d (%s)", w.Code, w.Body.String())
	}
	if w.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("cache-control %q", w.Header().Get("Cache-Control"))
	}
	if strings.Join(fx.memberArg, "|") != "room-1|org-bound|subject-1" {
		t.Fatalf("membership read for %v", fx.memberArg)
	}
	if len(fx.people.queries) != 2 {
		t.Fatalf("queries %d", len(fx.people.queries))
	}
	for _, q := range fx.people.queries {
		// The organization and the caller come from the run, never from the request's selector.
		if q.OrgID != "org-bound" || q.CallerID != "subject-1" || q.Q != "ada" || q.Limit != 50 {
			t.Fatalf("query %+v", q)
		}
	}
	if fx.people.queries[1].ExcludeMemberOf != "room-1" {
		t.Fatalf("scoped query %+v", fx.people.queries[1])
	}
	var body runDirectoryBody
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if len(body.Data.People) != 2 || !body.Data.People[0].InRoom || body.Data.People[1].InRoom {
		t.Fatalf("people %+v", body.Data.People)
	}
	if strings.Contains(w.Body.String(), "room-1") {
		t.Fatalf("the answer names a room: %s", w.Body.String())
	}
}

func TestRunDirectoryReportsZeroDataRetention(t *testing.T) {
	fx := newRunDirectoryFixture(t)
	fx.member.ZeroDataRetention = true
	w := runDirectoryCall(fx.server, executionCoreDirectoryCaller(), runDirectoryValidBody)
	var body runDirectoryBody
	if w.Code != http.StatusOK || json.Unmarshal(w.Body.Bytes(), &body) != nil || !body.Data.ZeroDataRetention {
		t.Fatalf("%d %s", w.Code, w.Body.String())
	}
	// Control drops names and emails itself under ZDR; ids and in_room stay.
	for _, leaked := range []string{"Ada", "Adam", "example.test"} {
		if strings.Contains(w.Body.String(), leaked) {
			t.Fatalf("%q leaked under ZDR: %s", leaked, w.Body.String())
		}
	}
	if len(body.Data.People) != 2 || body.Data.People[0].SubjectID != "u-ada" || !body.Data.People[0].InRoom ||
		!strings.Contains(w.Body.String(), `"names_withheld":true`) {
		t.Fatalf("people %s", w.Body.String())
	}
}

// fakeRunDirectoryReader stands in for the Space repository on the real
// membership path (readRunDirectoryMembership), so its errors are the ones the
// repository returns.
type fakeRunDirectoryReader struct {
	membership *spaces.CurrentMembership
	memberErr  error
	policy     spaces.EffectPolicy
	policyErr  error
	policyOrg  string
}

func (f *fakeRunDirectoryReader) ResolveCurrentUserMembership(_ context.Context, _, _, _ string) (*spaces.CurrentMembership, error) {
	return f.membership, f.memberErr
}

func (f *fakeRunDirectoryReader) ResolveOrgEffectPolicy(_ context.Context, orgID string) (spaces.EffectPolicy, error) {
	f.policyOrg = orgID
	return f.policy, f.policyErr
}

// useRunDirectoryReader routes the fixture through readRunDirectoryMembership.
func useRunDirectoryReader(fx *runDirectoryFixture, reader *fakeRunDirectoryReader) {
	fx.server.runDirectoryMembership = func(ctx context.Context, spaceRef, orgID, subjectID string) (runDirectoryMembership, error) {
		return readRunDirectoryMembership(ctx, reader, spaceRef, orgID, subjectID)
	}
}

// A viewer of a ZDR room without the agent-action entitlement reads the
// directory: the read needs membership only, and ZDR gives ids, not a refusal.
func TestRunDirectoryMembershipPathAdmitsAViewerOfAZeroRetentionRoom(t *testing.T) {
	fx := newRunDirectoryFixture(t)
	reader := &fakeRunDirectoryReader{
		membership: &spaces.CurrentMembership{SpaceRef: "room-1", OrgID: "org-bound", SubjectID: "subject-1", Role: spaces.RoleViewer},
		policy:     spaces.EffectPolicy{ZeroDataRetention: true},
	}
	useRunDirectoryReader(fx, reader)
	w := runDirectoryCall(fx.server, executionCoreDirectoryCaller(), runDirectoryValidBody)
	var body runDirectoryBody
	if w.Code != http.StatusOK || json.Unmarshal(w.Body.Bytes(), &body) != nil || !body.Data.ZeroDataRetention {
		t.Fatalf("%d %s", w.Code, w.Body.String())
	}
	if reader.policyOrg != "org-bound" || strings.Contains(w.Body.String(), "example.test") {
		t.Fatalf("policy org %q, body %s", reader.policyOrg, w.Body.String())
	}
}

// Typed refusals from the repository keep their own status and code; only an
// untyped failure is an outage.
func TestRunDirectoryMembershipPathMapsRefusals(t *testing.T) {
	viewer := &spaces.CurrentMembership{SpaceRef: "room-1", OrgID: "org-bound", SubjectID: "subject-1", Role: spaces.RoleViewer}
	cases := []struct {
		name   string
		reader *fakeRunDirectoryReader
		status int
		code   string
	}{
		{"not a member", &fakeRunDirectoryReader{memberErr: spaces.ErrNoCurrentMembership}, http.StatusForbidden, "not_authorized"},
		{"wrapped not a member", &fakeRunDirectoryReader{memberErr: errors.Join(errors.New("ctx"), spaces.ErrNoCurrentMembership)}, http.StatusForbidden, "not_authorized"},
		{"nil membership", &fakeRunDirectoryReader{}, http.StatusForbidden, "not_authorized"},
		{"role denial", &fakeRunDirectoryReader{memberErr: &spaces.Denial{Code: spaces.DenialRoleBelowFloor, RequiredRoles: []string{"editor"}}}, http.StatusForbidden, "space_role_below_floor"},
		{"entitlement denial", &fakeRunDirectoryReader{memberErr: &spaces.Denial{Code: spaces.DenialEntitlementOff, Entitlement: "agent_action"}}, http.StatusForbidden, "space_entitlement_off"},
		{"authority unavailable denial", &fakeRunDirectoryReader{memberErr: spaces.AuthorityUnavailableDenial("down")}, http.StatusServiceUnavailable, "space_authority_unavailable"},
		{"no policy row", &fakeRunDirectoryReader{membership: viewer, policyErr: spaces.ErrNoOrgEffectPolicy}, http.StatusForbidden, "space_policy_not_provisioned"},
		{"membership outage", &fakeRunDirectoryReader{memberErr: errors.New("db down")}, http.StatusServiceUnavailable, "directory_unavailable"},
		{"policy outage", &fakeRunDirectoryReader{membership: viewer, policyErr: errors.New("db down")}, http.StatusServiceUnavailable, "directory_unavailable"},
	}
	for _, tc := range cases {
		fx := newRunDirectoryFixture(t)
		useRunDirectoryReader(fx, tc.reader)
		w := runDirectoryCall(fx.server, executionCoreDirectoryCaller(), runDirectoryValidBody)
		if w.Code != tc.status || directoryErrorCode(t, w) != tc.code {
			t.Errorf("%s: %d %s", tc.name, w.Code, w.Body.String())
		}
		if len(fx.people.queries) != 0 {
			t.Errorf("%s: searched the directory after a refusal", tc.name)
		}
	}
}

func TestRunDirectoryFailsClosed(t *testing.T) {
	cases := []struct {
		name   string
		mutate func(*runDirectoryFixture)
		body   string
		status int
		code   string
	}{
		{"not a member any more", func(fx *runDirectoryFixture) { fx.memberErr = spaces.ErrNoCurrentMembership }, "", http.StatusForbidden, "not_authorized"},
		{"membership outage", func(fx *runDirectoryFixture) { fx.memberErr = errors.New("db down") }, "", http.StatusServiceUnavailable, "directory_unavailable"},
		{"session core outage", func(fx *runDirectoryFixture) { fx.authority.err = errors.New("down") }, "", http.StatusServiceUnavailable, "directory_unavailable"},
		{"terminal run", func(fx *runDirectoryFixture) { fx.authority.authority.RunStatus = "completed" }, "", http.StatusForbidden, "not_authorized"},
		{"row not admitted", func(fx *runDirectoryFixture) { fx.admitted = false }, "", http.StatusForbidden, "not_offered"},
		{"directory outage is not an empty list", func(fx *runDirectoryFixture) { fx.people.err = errors.New("down") }, "", http.StatusServiceUnavailable, "directory_unavailable"},
		{"subject left the org", func(fx *runDirectoryFixture) { fx.people.err = directory.ErrCallerNotInOrg }, "", http.StatusForbidden, "not_authorized"},
		{"unknown field", nil, `{"run_id":"run-1","org_id":"o","query":"ada","space_ref":"other-room"}`, http.StatusBadRequest, "invalid_request"},
		{"empty query", nil, `{"run_id":"run-1","org_id":"o","query":"  "}`, http.StatusBadRequest, "invalid_request"},
		{"long query", nil, `{"run_id":"run-1","org_id":"o","query":"` + strings.Repeat("a", 101) + `"}`, http.StatusBadRequest, "invalid_request"},
		{"no run", nil, `{"org_id":"o","query":"ada"}`, http.StatusBadRequest, "invalid_request"},
	}
	for _, tc := range cases {
		fx := newRunDirectoryFixture(t)
		if tc.mutate != nil {
			tc.mutate(fx)
		}
		body := tc.body
		if body == "" {
			body = runDirectoryValidBody
		}
		w := runDirectoryCall(fx.server, executionCoreDirectoryCaller(), body)
		if w.Code != tc.status || directoryErrorCode(t, w) != tc.code {
			t.Errorf("%s: %d %s", tc.name, w.Code, w.Body.String())
		}
	}
}

func TestRunDirectoryUnwiredIsUnavailable(t *testing.T) {
	w := runDirectoryCall(&Server{}, executionCoreDirectoryCaller(), runDirectoryValidBody)
	if w.Code != http.StatusServiceUnavailable || directoryErrorCode(t, w) != "directory_unavailable" {
		t.Fatalf("%d %s", w.Code, w.Body.String())
	}
}

// The real registry check never admits the action while the operator allowlist
// leaves it out, whether or not the golden row is vendored yet.
func TestRunDirectoryRealAdmissionNeedsTheAllowlist(t *testing.T) {
	t.Setenv("CONTROL_SPACE_SELF_API_ACTIONS", "space.thread.rename")
	for _, role := range []string{spaces.RoleViewer, spaces.RoleOwner} {
		if spaces.RunActionReadAdmitted("space.directory.resolve", role) {
			t.Fatalf("admitted without the allowlist at %s", role)
		}
	}
}
