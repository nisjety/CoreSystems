//go:build integration

package handler

// WS14-S4 share routes with the owner proof, which reads the document. Control
// is an httptest stub; documents are rows in the throwaway database.

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/userauthz"
)

const shareServiceToken = "0123456789abcdef0123456789abcdef"

type controlStub struct {
	mu          sync.Mutex
	srv         *httptest.Server
	calls       []string
	grants      []userauthz.Grant
	grantStatus int
	grantBody   string
	visibleIDs  []string
}

func newControlStub(t *testing.T) *controlStub {
	t.Helper()
	c := &controlStub{grantStatus: http.StatusOK}
	c.srv = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		c.mu.Lock()
		defer c.mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		switch {
		case r.Method == http.MethodGet && strings.HasSuffix(r.URL.Path, "/authz/grants"):
			c.calls = append(c.calls, "list")
			_ = json.NewEncoder(w).Encode(map[string]any{"grants": c.grants})
		case r.Method == http.MethodPost && strings.HasSuffix(r.URL.Path, "/authz/grant"):
			c.calls = append(c.calls, "grant")
			w.WriteHeader(c.grantStatus)
			body := c.grantBody
			if body == "" {
				body = `{"grant_id":"g-new","subject_type":"user","subject_id":"x","role":"view","granted_by":"owner-1","granted_at":"2026-11-02T06:00:00Z"}`
			}
			_, _ = w.Write([]byte(body))
		case r.Method == http.MethodDelete && strings.HasSuffix(r.URL.Path, "/authz/grant"):
			c.calls = append(c.calls, "revoke:"+r.URL.Query().Get("subject_type"))
			_, _ = w.Write([]byte(`{}`))
		case r.Method == http.MethodGet && strings.HasSuffix(r.URL.Path, "/authz/visible"):
			c.calls = append(c.calls, "visible")
			_ = json.NewEncoder(w).Encode(map[string]any{"ids": c.visibleIDs, "all_org": false})
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	t.Cleanup(c.srv.Close)
	return c
}

func (c *controlStub) snapshot() []string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]string(nil), c.calls...)
}

func (c *controlStub) reset() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.calls = nil
}

type shareLive struct {
	t       *testing.T
	auth    *testAuth
	control *controlStub
	router  http.Handler
	repo    *repo.DocumentRepo
	docs    map[string]string
}

func newShareLive(t *testing.T, enabled bool) *shareLive {
	t.Helper()
	r, _ := liveDB(t)
	auth := newTestAuth(t)
	control := newControlStub(t)
	client := userauthz.New(control.srv.URL, shareServiceToken)
	h := NewDocumentHandler(r, client).WithShares(SharesConfig{Enabled: enabled, Grants: client})
	router := chi.NewRouter()
	router.Use(auth.wrap)
	router.Get("/v1/documents/shared-with-me", h.SharedWithMe)
	router.Get("/v1/documents/{documentID}/shares", h.ListShares)
	router.Post("/v1/documents/{documentID}/shares", h.CreateShare)
	router.Delete("/v1/documents/{documentID}/shares/{subjectID}", h.DeleteShare)
	return &shareLive{t: t, auth: auth, control: control, router: router, repo: r, docs: map[string]string{}}
}

func (s *shareLive) doc(name, owner, visibility string) string {
	s.t.Helper()
	res, err := s.repo.Create(context.Background(), model.CreateDocumentInput{
		OrgID: testOrg, Source: "s", Type: "note", Title: name, Content: "body", OwnerID: owner, Visibility: visibility,
	})
	if err != nil {
		s.t.Fatal(err)
	}
	s.docs[name] = res.Document.DocumentID
	return res.Document.DocumentID
}

func (s *shareLive) call(token, method, path, body string) *httptest.ResponseRecorder {
	s.t.Helper()
	return shareCall(s.router, method, path, token, body)
}

func TestSharesNonOwnerAndOrgVisibleNonOwnedAre404OnAllThreeRoutes(t *testing.T) {
	s := newShareLive(t, true)
	private := s.doc("private", "owner-1", "private")
	orgDoc := s.doc("org-visible", "owner-1", "org")
	stranger := s.auth.userToken(testOrg, "stranger-1", false)
	for _, id := range []string{private, orgDoc} {
		for _, tc := range []struct{ method, path, body string }{
			{http.MethodGet, "/v1/documents/" + id + "/shares", ""},
			{http.MethodPost, "/v1/documents/" + id + "/shares", `{"subject_id":"user-2"}`},
			{http.MethodDelete, "/v1/documents/" + id + "/shares/user-2", ""},
		} {
			if rec := s.call(stranger, tc.method, tc.path, tc.body); rec.Code != http.StatusNotFound {
				t.Errorf("%s %s: %d %s", tc.method, tc.path, rec.Code, rec.Body.String())
			}
		}
	}
	if calls := s.control.snapshot(); len(calls) != 0 {
		t.Fatalf("Control was called for a non-owner: %v", calls)
	}
}

func TestSharesOwnerListsAddsAndRemovesOnlyUserGrants(t *testing.T) {
	s := newShareLive(t, true)
	id := s.doc("mine", "owner-1", "private")
	s.control.grants = []userauthz.Grant{
		{GrantID: "g1", SubjectType: "user", SubjectID: "user-2", Role: "view", GrantedBy: "owner-1"},
		{GrantID: "g2", SubjectType: "space", SubjectID: testSpace, Role: "view", GrantedBy: "owner-1"},
	}
	owner := s.auth.userToken(testOrg, "owner-1", false)

	list := s.call(owner, http.MethodGet, "/v1/documents/"+id+"/shares", "")
	var listed struct {
		Shares []shareView `json:"shares"`
	}
	if list.Code != http.StatusOK || json.Unmarshal(list.Body.Bytes(), &listed) != nil || len(listed.Shares) != 2 || listed.Shares[1].SubjectType != "space" {
		t.Fatalf("list: %d %s", list.Code, list.Body.String())
	}

	if rec := s.call(owner, http.MethodPost, "/v1/documents/"+id+"/shares", `{"subject_id":"user-3","role":"VIEW"}`); rec.Code != http.StatusOK {
		t.Fatalf("add: %d %s", rec.Code, rec.Body.String())
	}
	if rec := s.call(owner, http.MethodPost, "/v1/documents/"+id+"/shares", `{"subject_id":"user-4"}`); rec.Code != http.StatusOK {
		t.Fatalf("add with default role: %d %s", rec.Code, rec.Body.String())
	}

	s.control.reset()
	if rec := s.call(owner, http.MethodDelete, "/v1/documents/"+id+"/shares/"+testSpace, ""); rec.Code != http.StatusOK {
		t.Fatalf("remove: %d %s", rec.Code, rec.Body.String())
	}
	if calls := s.control.snapshot(); len(calls) != 1 || calls[0] != "revoke:user" {
		t.Fatalf("remove asked Control %v, want exactly revoke:user (a Space row is never reachable here)", calls)
	}
}

func TestSharesOrgWriteAllScopeIsAllowedOnAVisibleDocument(t *testing.T) {
	s := newShareLive(t, true)
	orgDoc := s.doc("org doc", "owner-1", "org")
	private := s.doc("private doc", "owner-1", "private")
	admin := s.auth.userToken(testOrg, "admin-1", false, "org:data:write_all")
	if rec := s.call(admin, http.MethodPost, "/v1/documents/"+orgDoc+"/shares", `{"subject_id":"user-2"}`); rec.Code != http.StatusOK {
		t.Fatalf("write_all on a visible document: %d %s", rec.Code, rec.Body.String())
	}
	// The scope does not widen what the viewer can see: a private document of
	// someone else is still a 404.
	if rec := s.call(admin, http.MethodPost, "/v1/documents/"+private+"/shares", `{"subject_id":"user-2"}`); rec.Code != http.StatusNotFound {
		t.Fatalf("write_all on someone's private document: %d", rec.Code)
	}
}

func TestSharesRefuseEditRoleSelfShareAndBadBodyWithoutCallingControl(t *testing.T) {
	s := newShareLive(t, true)
	id := s.doc("mine", "owner-1", "private")
	owner := s.auth.userToken(testOrg, "owner-1", false)
	path := "/v1/documents/" + id + "/shares"

	for _, role := range []string{"edit", "admin", "owner"} {
		rec := s.call(owner, http.MethodPost, path, fmt.Sprintf(`{"subject_id":"user-2","role":%q}`, role))
		if rec.Code != http.StatusBadRequest || decodeError(t, rec)["code"] != "role_not_supported" {
			t.Errorf("role %s: %d %s", role, rec.Code, rec.Body.String())
		}
	}
	for _, subject := range []string{"owner-1"} {
		rec := s.call(owner, http.MethodPost, path, fmt.Sprintf(`{"subject_id":%q}`, subject))
		if rec.Code != http.StatusConflict || decodeError(t, rec)["code"] != "self_share" {
			t.Errorf("subject %s: %d %s", subject, rec.Code, rec.Body.String())
		}
	}
	for _, body := range []string{`{"subject_id":""}`, `{"subject_id":"has space"}`, `{"subject_id":"u","extra":1}`, `not json`} {
		if rec := s.call(owner, http.MethodPost, path, body); rec.Code != http.StatusBadRequest || decodeError(t, rec)["code"] != "invalid_request" {
			t.Errorf("body %s: %d %s", body, rec.Code, rec.Body.String())
		}
	}
	// A write_all actor sharing someone else's visible document with that
	// document's OWNER is also a self share.
	orgDoc := s.doc("org doc", "owner-2", "org")
	admin := s.auth.userToken(testOrg, "admin-1", false, "org:data:write_all")
	if rec := s.call(admin, http.MethodPost, "/v1/documents/"+orgDoc+"/shares", `{"subject_id":"owner-2"}`); rec.Code != http.StatusConflict {
		t.Errorf("share with the owner: %d %s", rec.Code, rec.Body.String())
	}
	for _, c := range s.control.snapshot() {
		if c == "grant" {
			t.Fatal("a refused share reached Control's grant write")
		}
	}
}

func TestSharesUserGrantCapIsEnforcedButReshareOfAnExistingSubjectIsAllowed(t *testing.T) {
	s := newShareLive(t, true)
	id := s.doc("popular", "owner-1", "private")
	for i := 0; i < maxSharesPerDocument; i++ {
		s.control.grants = append(s.control.grants, userauthz.Grant{SubjectType: "user", SubjectID: fmt.Sprintf("user-%d", i), Role: "view"})
	}
	// Space rows do not count toward the user cap.
	s.control.grants = append(s.control.grants, userauthz.Grant{SubjectType: "space", SubjectID: testSpace, Role: "view"})
	owner := s.auth.userToken(testOrg, "owner-1", false)
	path := "/v1/documents/" + id + "/shares"

	rec := s.call(owner, http.MethodPost, path, `{"subject_id":"brand-new"}`)
	body := decodeError(t, rec)
	details, _ := body["details"].(map[string]any)
	if rec.Code != http.StatusConflict || body["code"] != "share_limit_reached" || details["max_shares"] != float64(maxSharesPerDocument) {
		t.Fatalf("cap: %d %v", rec.Code, body)
	}
	if rec := s.call(owner, http.MethodPost, path, `{"subject_id":"user-7"}`); rec.Code != http.StatusOK {
		t.Fatalf("re-share of an existing subject at the cap: %d %s", rec.Code, rec.Body.String())
	}
}

func TestSharesControlFailuresMapToPassThroughCodeOr503(t *testing.T) {
	s := newShareLive(t, true)
	id := s.doc("mine", "owner-1", "private")
	owner := s.auth.userToken(testOrg, "owner-1", false)
	path := "/v1/documents/" + id + "/shares"
	post := func(status int, body string) *httptest.ResponseRecorder {
		s.control.grantStatus, s.control.grantBody = status, body
		return s.call(owner, http.MethodPost, path, `{"subject_id":"user-2"}`)
	}

	rec := post(http.StatusBadRequest, `{"code":"member_not_in_organization"}`)
	if rec.Code != http.StatusBadRequest || decodeError(t, rec)["code"] != "member_not_in_organization" {
		t.Fatalf("400 with code: %d %s", rec.Code, rec.Body.String())
	}
	rec = post(http.StatusBadRequest, `{}`)
	if rec.Code != http.StatusBadRequest || decodeError(t, rec)["code"] != "share_invalid" {
		t.Fatalf("400 without code: %d %s", rec.Code, rec.Body.String())
	}
	// 403 and 404 mean the owner-asserted mode is not open yet: unavailable,
	// never presented to the user as a permission verdict.
	for _, status := range []int{http.StatusForbidden, http.StatusNotFound, http.StatusInternalServerError, http.StatusBadGateway} {
		rec := post(status, `{"code":"owner_assertion_required"}`)
		if rec.Code != http.StatusServiceUnavailable || decodeError(t, rec)["code"] != "sharing_unavailable" {
			t.Errorf("control %d: %d %s", status, rec.Code, rec.Body.String())
		}
	}
}

func TestSharedWithMeWorksWithTheFlagOffAndExcludesOwnAndDeletedDocuments(t *testing.T) {
	s := newShareLive(t, false)
	fromA := s.doc("from A", "user-a", "private")
	own := s.doc("own", "user-b", "private")
	gone := s.doc("gone", "user-a", "private")
	if err := s.repo.SoftDelete(context.Background(), testOrg, gone, "user-a"); err != nil {
		t.Fatal(err)
	}
	s.control.visibleIDs = []string{fromA, own, gone}

	rec := s.call(s.auth.userToken(testOrg, "user-b", false), http.MethodGet, "/v1/documents/shared-with-me", "")
	var out struct {
		IDs       []string         `json:"ids"`
		Documents []map[string]any `json:"documents"`
		AllOrg    bool             `json:"all_org"`
	}
	if rec.Code != http.StatusOK || json.Unmarshal(rec.Body.Bytes(), &out) != nil {
		t.Fatalf("%d %s", rec.Code, rec.Body.String())
	}
	if len(out.IDs) != 1 || out.IDs[0] != fromA || out.AllOrg {
		t.Fatalf("shared-with-me = %+v, want only A's live document and all_org false", out)
	}
	for _, d := range out.Documents {
		if _, hasContent := d["content"]; hasContent {
			t.Fatal("shared-with-me must not return document content")
		}
	}
	// The write routes stay 404 with the flag off.
	if rec := s.call(s.auth.userToken(testOrg, "user-a", false), http.MethodGet, "/v1/documents/"+fromA+"/shares", ""); rec.Code != http.StatusNotFound {
		t.Fatalf("shares route with the flag off: %d", rec.Code)
	}
}
