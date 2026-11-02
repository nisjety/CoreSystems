package http

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/directory"
)

// WS04-S3: GET /api/v1/internal/directory/people. A fake directory stands in
// for Postgres; the real route (registered through the P-UC1 seam) and the
// real guard run.

type fakePeopleDirectory struct {
	result  directory.PeopleResult
	err     error
	queries []directory.PeopleQuery
}

func (f *fakePeopleDirectory) SearchPeople(_ context.Context, q directory.PeopleQuery) (directory.PeopleResult, error) {
	f.queries = append(f.queries, q)
	return f.result, f.err
}

const directoryPeoplePath = "/api/v1/internal/directory/people"

func directoryCall(s *Server, caller orgPolicyCaller, target string) *httptest.ResponseRecorder {
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
	v1 := engine.Group("/api/v1")
	internal := v1.Group("/internal")
	for _, register := range featureRouteRegistrars {
		register(s, v1, internal)
	}
	w := httptest.NewRecorder()
	engine.ServeHTTP(w, httptest.NewRequest(http.MethodGet, target, nil))
	return w
}

func directoryReader() orgPolicyCaller {
	return gatewayAdmin("directory:read:self")
}

func directoryErrorCode(t *testing.T, w *httptest.ResponseRecorder) string {
	t.Helper()
	var body struct {
		Error struct {
			Code    string         `json:"code"`
			Details map[string]any `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("not a typed error: %v (%s)", err, w.Body.String())
	}
	if body.Error.Details == nil {
		t.Fatalf("details must be an object: %s", w.Body.String())
	}
	return body.Error.Code
}

func TestDirectoryRouteRegistersThroughTheSeam(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	v1 := engine.Group("/api/v1")
	internal := v1.Group("/internal")
	for _, register := range featureRouteRegistrars {
		register(&Server{}, v1, internal)
	}
	if got := routeCounts(engine.Routes())["GET "+directoryPeoplePath]; got != 1 {
		t.Fatalf("GET %s registered %d times", directoryPeoplePath, got)
	}
}

func TestDirectoryRouteNeedsGatewayDelegationAndTheScope(t *testing.T) {
	cases := []struct {
		name   string
		caller orgPolicyCaller
	}{
		{"no delegation", func() orgPolicyCaller { c := directoryReader(); c.delegated = false; return c }()},
		{"another service", func() orgPolicyCaller { c := directoryReader(); c.serviceID = "integration-core"; return c }()},
		{"not a service principal", func() orgPolicyCaller { c := directoryReader(); c.authMethod = "user"; return c }()},
		{"missing scope", gatewayAdmin("users:read:self")},
		{"a spaces scope does not open the directory", gatewayAdmin("spaces:resolve")},
		{"no user", func() orgPolicyCaller { c := directoryReader(); c.userID = ""; return c }()},
		{"no org", func() orgPolicyCaller { c := directoryReader(); c.orgID = ""; return c }()},
		{"no auth context", orgPolicyCaller{omitContext: true}},
	}
	for _, tc := range cases {
		store := &fakePeopleDirectory{}
		s := &Server{}
		s.SetPeopleDirectory(store)
		w := directoryCall(s, tc.caller, directoryPeoplePath)
		if w.Code != http.StatusForbidden {
			t.Errorf("%s: want 403, got %d (%s)", tc.name, w.Code, w.Body.String())
			continue
		}
		if code := directoryErrorCode(t, w); code != "directory_delegation_required" {
			t.Errorf("%s: code %q", tc.name, code)
		}
		if len(store.queries) != 0 {
			t.Errorf("%s: reached the directory", tc.name)
		}
	}
}

func TestDirectoryRouteTakesOrgAndCallerOnlyFromTheDelegation(t *testing.T) {
	store := &fakePeopleDirectory{result: directory.PeopleResult{
		People:     []directory.Person{{SubjectID: "user-2", DisplayName: "Ada", Email: "ada@example.com", IsSelf: false}},
		NextCursor: "cursor-2",
	}}
	s := &Server{}
	s.SetPeopleDirectory(store)

	// Org and caller in the query string must be ignored.
	w := directoryCall(s, directoryReader(),
		directoryPeoplePath+"?q=ada&limit=7&cursor=abc&exclude_member_of=space-9&org_id=org-evil&user_id=user-evil&caller_id=user-evil")
	if w.Code != http.StatusOK {
		t.Fatalf("want 200, got %d (%s)", w.Code, w.Body.String())
	}
	if len(store.queries) != 1 {
		t.Fatalf("queries = %d", len(store.queries))
	}
	q := store.queries[0]
	if q.OrgID != "org-1" || q.CallerID != "user-1" {
		t.Fatalf("org/caller not from the delegation: %+v", q)
	}
	if q.Q != "ada" || q.Limit != 7 || q.Cursor != "abc" || q.ExcludeMemberOf != "space-9" {
		t.Fatalf("query parameters drifted: %+v", q)
	}
	if w.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("Cache-Control = %q, want no-store", w.Header().Get("Cache-Control"))
	}
	var body struct {
		Data struct {
			People     []directory.Person `json:"people"`
			NextCursor *string            `json:"next_cursor"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if len(body.Data.People) != 1 || body.Data.People[0].SubjectID != "user-2" || body.Data.NextCursor == nil || *body.Data.NextCursor != "cursor-2" {
		t.Fatalf("envelope = %s", w.Body.String())
	}

	// The last page has next_cursor null, and an empty page is [] not null.
	store.result = directory.PeopleResult{People: []directory.Person{}}
	w = directoryCall(s, directoryReader(), directoryPeoplePath)
	if w.Code != http.StatusOK || !jsonContains(w, `"next_cursor":null`) || !jsonContains(w, `"people":[]`) {
		t.Fatalf("last page: %d %s", w.Code, w.Body.String())
	}
	if store.queries[1].Limit != 0 {
		t.Fatalf("an absent limit must reach the store as 0 (its default), got %d", store.queries[1].Limit)
	}
}

func jsonContains(w *httptest.ResponseRecorder, fragment string) bool {
	return strings.Contains(w.Body.String(), fragment)
}

func TestDirectoryRouteErrorMapping(t *testing.T) {
	cases := []struct {
		name       string
		err        error
		target     string
		wantStatus int
		wantCode   string
	}{
		{"invalid request from the store", fmt.Errorf("%w: q is too long", directory.ErrInvalidRequest), directoryPeoplePath, 400, "invalid_request"},
		{"caller not in the org", directory.ErrCallerNotInOrg, directoryPeoplePath, 403, "member_not_in_organization"},
		{"wrapped caller not in the org", fmt.Errorf("lookup: %w", directory.ErrCallerNotInOrg), directoryPeoplePath, 403, "member_not_in_organization"},
		{"store failure", errors.New("db exploded"), directoryPeoplePath, 503, "directory_unavailable"},
		{"non numeric limit", nil, directoryPeoplePath + "?limit=lots", 400, "invalid_request"},
	}
	for _, tc := range cases {
		store := &fakePeopleDirectory{err: tc.err}
		s := &Server{}
		s.SetPeopleDirectory(store)
		w := directoryCall(s, directoryReader(), tc.target)
		if w.Code != tc.wantStatus {
			t.Errorf("%s: status %d, want %d (%s)", tc.name, w.Code, tc.wantStatus, w.Body.String())
			continue
		}
		if code := directoryErrorCode(t, w); code != tc.wantCode {
			t.Errorf("%s: code %q, want %q", tc.name, code, tc.wantCode)
		}
		if tc.name == "store failure" && jsonContains(w, "exploded") {
			t.Errorf("a raw store error leaked: %s", w.Body.String())
		}
		if tc.name == "non numeric limit" && len(store.queries) != 0 {
			t.Errorf("a malformed limit reached the store")
		}
	}
}

func TestDirectoryRouteIsUnavailableWhenUnwired(t *testing.T) {
	w := directoryCall(&Server{}, directoryReader(), directoryPeoplePath)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("want 503, got %d (%s)", w.Code, w.Body.String())
	}
	if code := directoryErrorCode(t, w); code != "directory_unavailable" {
		t.Fatalf("code = %q", code)
	}
	// Unwired but unauthorised is still a 403, never a 503 that confirms the route.
	caller := directoryReader()
	caller.delegated = false
	if w := directoryCall(&Server{}, caller, directoryPeoplePath); w.Code != http.StatusForbidden {
		t.Fatalf("unauthorised and unwired: want 403, got %d", w.Code)
	}
}
