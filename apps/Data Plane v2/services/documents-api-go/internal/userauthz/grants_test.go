package userauthz

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// WS14-S4: the owner-asserted grant calls. Each one must carry the actor's
// bearer and a v2 delegation whose operation, resource id and body digest
// verify against Control's canonical form.

const grantsToken = "0123456789abcdef0123456789abcdef"

var grantsNow = time.Date(2026, 11, 2, 6, 0, 0, 0, time.UTC)

// verifyDelegation re-derives the signature exactly as Control's
// service_delegation.go does and fails the test on any mismatch.
func verifyDelegation(t *testing.T, r *http.Request, body []byte, wantOperation, wantResource string) {
	t.Helper()
	if r.Header.Get("Authorization") != "Bearer actor-token" {
		t.Errorf("Authorization = %q, want the actor's bearer", r.Header.Get("Authorization"))
	}
	if r.Header.Get("X-Service-Id") != "documents-api" || r.Header.Get("X-Service-Token") != grantsToken {
		t.Error("service identity missing")
	}
	if r.Header.Get("X-User-Id") != "actor-1" || r.Header.Get("X-Org-Id") != "org-1" {
		t.Error("delegated actor or tenant not bound")
	}
	if got := r.Header.Get("X-Delegation-Operation"); got != wantOperation {
		t.Errorf("operation = %q, want %q", got, wantOperation)
	}
	if got := r.Header.Get("X-Delegation-Resource-Id"); got != wantResource {
		t.Errorf("resource id = %q, want %q", got, wantResource)
	}
	if r.Header.Get("X-Delegation-Version") != "v2" || r.Header.Get("X-Delegation-Resource-Type") != "document" || r.Header.Get("X-Delegation-ZDR") != "true" {
		t.Error("delegation envelope is not a v2 document delegation")
	}
	sum := sha256.Sum256(body)
	digest := base64.RawURLEncoding.EncodeToString(sum[:])
	if got := r.Header.Get("X-Delegation-Body-SHA256"); got != digest {
		t.Errorf("body digest = %q, want %q", got, digest)
	}
	nonce := r.Header.Get("X-Delegation-Nonce")
	if len(nonce) < 16 {
		t.Error("delegation nonce missing")
	}
	canonical := strings.Join([]string{
		"v2", "documents-api", "user-core", grantsNow.Format(time.RFC3339),
		r.Method, r.URL.RequestURI(), "actor-1", "org-1", wantOperation,
		"document", wantResource, r.Header.Get("X-Delegation-Reason"), "true", nonce, digest,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(grantsToken))
	_, _ = mac.Write([]byte(canonical))
	if want := base64.RawURLEncoding.EncodeToString(mac.Sum(nil)); r.Header.Get("X-Delegation-Signature") != want {
		t.Error("delegation signature does not verify against the canonical form")
	}
}

func newGrantsClient(url string) *Client {
	client := New(url, grantsToken)
	client.now = func() time.Time { return grantsNow }
	return client
}

func TestGrantPostsViewGrantSignedOverTheBody(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		if r.Method != http.MethodPost || r.URL.Path != "/api/v1/internal/authz/grant" {
			t.Errorf("request = %s %s", r.Method, r.URL.Path)
		}
		verifyDelegation(t, r, body, "authz:grant", "doc-1")
		var sent map[string]string
		if err := json.Unmarshal(body, &sent); err != nil {
			t.Fatalf("body: %v", err)
		}
		want := map[string]string{"org_id": "org-1", "resource_type": "document", "resource_id": "doc-1", "subject_id": "user-2", "subject_type": "user", "role": "view"}
		for k, v := range want {
			if sent[k] != v {
				t.Errorf("body %s = %q, want %q", k, sent[k], v)
			}
		}
		if _, forged := sent["granted_by"]; forged {
			t.Error("granted_by must come from the delegation, never the body")
		}
		_ = json.NewEncoder(w).Encode(Grant{GrantID: "g-1", SubjectType: "user", SubjectID: "user-2", Role: "view", GrantedBy: "actor-1", GrantedAt: "2026-11-02T06:00:00Z"})
	}))
	defer server.Close()

	grant, err := newGrantsClient(server.URL).Grant(context.Background(), "org-1", "actor-1", "Bearer actor-token", "doc-1", "user-2", "view")
	if err != nil {
		t.Fatalf("Grant: %v", err)
	}
	if grant.GrantID != "g-1" || grant.GrantedBy != "actor-1" {
		t.Fatalf("grant = %+v", grant)
	}
}

func TestRevokeDeletesOnlyAUserSubjectWithEmptyBodyDigest(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodDelete || r.URL.Path != "/api/v1/internal/authz/grant" {
			t.Errorf("request = %s %s", r.Method, r.URL.Path)
		}
		verifyDelegation(t, r, nil, "authz:revoke", "doc-1")
		q := r.URL.Query()
		if q.Get("subject_type") != "user" || q.Get("subject_id") != "user-2" || q.Get("resource_id") != "doc-1" || q.Get("org_id") != "org-1" {
			t.Errorf("query = %v", q)
		}
		w.WriteHeader(http.StatusOK)
	}))
	defer server.Close()
	if err := newGrantsClient(server.URL).Revoke(context.Background(), "org-1", "actor-1", "Bearer actor-token", "doc-1", "user-2"); err != nil {
		t.Fatalf("Revoke: %v", err)
	}
}

func TestListGrantsReadsUserAndSpaceRows(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet || r.URL.Path != "/api/v1/internal/authz/grants" {
			t.Errorf("request = %s %s", r.Method, r.URL.Path)
		}
		verifyDelegation(t, r, nil, "authz:grants", "doc-1")
		_ = json.NewEncoder(w).Encode(map[string]any{"grants": []Grant{
			{GrantID: "g-1", SubjectType: "user", SubjectID: "user-2", Role: "view"},
			{GrantID: "g-2", SubjectType: "space", SubjectID: "space:org-1:room-7", Role: "view"},
		}})
	}))
	defer server.Close()
	grants, err := newGrantsClient(server.URL).ListGrants(context.Background(), "org-1", "actor-1", "Bearer actor-token", "doc-1")
	if err != nil {
		t.Fatalf("ListGrants: %v", err)
	}
	if len(grants) != 2 || grants[1].SubjectType != "space" {
		t.Fatalf("grants = %+v", grants)
	}
}

func TestListVisibleSpaceUsesTheSpaceRefAsDelegationResource(t *testing.T) {
	const spaceRef = "space:org-1:room-7"
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet || r.URL.Path != "/api/v1/internal/authz/space-visible" {
			t.Errorf("request = %s %s", r.Method, r.URL.Path)
		}
		verifyDelegation(t, r, nil, "authz:space-visible", spaceRef)
		q := r.URL.Query()
		if q.Get("space_ref") != spaceRef || q.Get("subject_id") != "actor-1" || q.Get("resource_type") != "document" {
			t.Errorf("query = %v", q)
		}
		_, _ = w.Write([]byte(`{"ids":["doc-1","doc-2"],"space_ref":"` + spaceRef + `","audience_revision":7}`))
	}))
	defer server.Close()
	visible, err := newGrantsClient(server.URL).ListVisibleSpace(context.Background(), "org-1", "actor-1", "Bearer actor-token", spaceRef)
	if err != nil {
		t.Fatalf("ListVisibleSpace: %v", err)
	}
	if len(visible.IDs) != 2 || visible.SpaceRef != spaceRef || string(visible.AudienceRevision) != "7" {
		t.Fatalf("visible = %+v", visible)
	}
}

func TestNon200BecomesStatusErrorWithControlsCode(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Query().Get("resource_id") {
		case "bad-role":
			w.WriteHeader(http.StatusBadRequest)
			_, _ = w.Write([]byte(`{"error":"role not supported","code":"role_not_supported"}`))
		case "no-code":
			w.WriteHeader(http.StatusBadGateway)
			_, _ = w.Write([]byte(`<html>`))
		default:
			w.WriteHeader(http.StatusForbidden)
			_, _ = w.Write([]byte(`{"code":"owner_assertion_required"}`))
		}
	}))
	defer server.Close()
	client := newGrantsClient(server.URL)
	ctx := context.Background()

	_, err := client.ListGrants(ctx, "org-1", "actor-1", "Bearer actor-token", "bad-role")
	var statusErr *StatusError
	if !errors.As(err, &statusErr) || statusErr.Status != 400 || statusErr.Code != "role_not_supported" || statusErr.Message != "role not supported" {
		t.Fatalf("err = %#v", err)
	}
	_, err = client.ListGrants(ctx, "org-1", "actor-1", "Bearer actor-token", "no-code")
	if !errors.As(err, &statusErr) || statusErr.Status != 502 || statusErr.Code != "" {
		t.Fatalf("no-code err = %#v", err)
	}
	_, err = client.ListVisibleSpace(ctx, "org-1", "actor-1", "Bearer actor-token", "space:x")
	if !errors.As(err, &statusErr) || statusErr.Status != 403 || statusErr.Code != "owner_assertion_required" {
		t.Fatalf("403 err = %#v", err)
	}
}

func TestGrantCallsRefuseMissingProofAndConfig(t *testing.T) {
	client := newGrantsClient("http://127.0.0.1:1")
	ctx := context.Background()
	for _, authorization := range []string{"", "Bearer ", "Bearer t\r\nX-Forged: 1"} {
		if _, err := client.ListGrants(ctx, "org-1", "actor-1", authorization, "doc-1"); err == nil {
			t.Fatalf("authorization %q accepted", authorization)
		}
	}
	if _, err := client.ListGrants(ctx, "", "actor-1", "Bearer t", "doc-1"); err == nil {
		t.Fatal("empty org accepted")
	}
	if _, err := client.ListGrants(ctx, "org-1", " ", "Bearer t", "doc-1"); err == nil {
		t.Fatal("empty user accepted")
	}
	if _, err := New("", grantsToken).ListGrants(ctx, "o", "u", "Bearer t", "d"); !errors.Is(err, ErrNotConfigured) {
		t.Fatalf("no url = %v", err)
	}
	if _, err := New("http://127.0.0.1:1", "").ListGrants(ctx, "o", "u", "Bearer t", "d"); !errors.Is(err, ErrNotConfigured) {
		t.Fatalf("no token = %v", err)
	}
}

func TestListVisibleDocumentsStillSignsAuthzVisibleWithEmptyDigest(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("X-Delegation-Operation") != "authz:visible" || r.Header.Get("X-Delegation-Resource-Id") != "" {
			t.Errorf("operation/resource = %q / %q", r.Header.Get("X-Delegation-Operation"), r.Header.Get("X-Delegation-Resource-Id"))
		}
		empty := sha256.Sum256(nil)
		if r.Header.Get("X-Delegation-Body-SHA256") != base64.RawURLEncoding.EncodeToString(empty[:]) {
			t.Error("authz:visible must keep signing the empty body digest")
		}
		_, _ = w.Write([]byte(`{"ids":["doc-9"],"all_org":false}`))
	}))
	defer server.Close()
	ids, err := newGrantsClient(server.URL).ListVisibleDocuments(context.Background(), "org-1", "actor-1", "Bearer actor-token")
	if err != nil || len(ids) != 1 || ids[0] != "doc-9" {
		t.Fatalf("ids=%v err=%v", ids, err)
	}
}
