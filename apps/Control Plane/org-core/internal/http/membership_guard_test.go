package http

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"

	"github.com/gin-gonic/gin"
)

// fakeAuthorizer is a test double for memberAuthorizer. It records the args it
// was called with and returns the configured result, so the guard can be tested
// without a database. The mutex keeps it race-clean under -race even though
// httptest.ServeHTTP is synchronous.
type fakeAuthorizer struct {
	mu        sync.Mutex
	member    bool
	err       error
	gotOrgID  string
	gotUserID string
	called    bool
}

func (f *fakeAuthorizer) IsActiveMember(_ context.Context, orgID, userID string) (bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.called = true
	f.gotOrgID = orgID
	f.gotUserID = userID
	return f.member, f.err
}

// snapshot returns the recorded call state under the lock so test-goroutine
// reads never race the guard-goroutine writes under -race.
func (f *fakeAuthorizer) snapshot() (called bool, orgID, userID string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.called, f.gotOrgID, f.gotUserID
}

// newGuardedRouter wires a single POST /orgs/:id route protected by the
// membership guard. The terminal handler returns 200 so we can distinguish
// "guard passed" (200) from "guard denied" (403/400).
func newGuardedRouter(authz memberAuthorizer) *gin.Engine {
	gin.SetMode(gin.TestMode)
	r := gin.New()
	r.POST("/orgs/:id/plan", requireActiveMembership(authz), func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{"ok": true})
	})
	return r
}

func doRequest(t *testing.T, r *gin.Engine, orgID, userID string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, "/orgs/"+orgID+"/plan", http.NoBody)
	if userID != "" {
		req.Header.Set("x-user-id", userID)
	}
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)
	return w
}

func TestRequireActiveMembership(t *testing.T) {
	tests := []struct {
		name       string
		orgID      string
		userID     string
		authz      *fakeAuthorizer
		wantStatus int
		wantCalled bool
	}{
		{
			name:       "active member is allowed",
			orgID:      "org_123",
			userID:     "user_abc",
			authz:      &fakeAuthorizer{member: true},
			wantStatus: http.StatusOK,
			wantCalled: true,
		},
		{
			name:       "non-member is forbidden",
			orgID:      "org_123",
			userID:     "user_intruder",
			authz:      &fakeAuthorizer{member: false},
			wantStatus: http.StatusForbidden,
			wantCalled: true,
		},
		{
			name:       "missing user id is forbidden without a lookup",
			orgID:      "org_123",
			userID:     "",
			authz:      &fakeAuthorizer{member: true},
			wantStatus: http.StatusForbidden,
			wantCalled: false,
		},
		{
			name:       "lookup error fails closed with forbidden",
			orgID:      "org_123",
			userID:     "user_abc",
			authz:      &fakeAuthorizer{err: errors.New("db down")},
			wantStatus: http.StatusForbidden,
			wantCalled: true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			r := newGuardedRouter(tc.authz)
			w := doRequest(t, r, tc.orgID, tc.userID)

			called, gotOrgID, gotUserID := tc.authz.snapshot()
			if w.Code != tc.wantStatus {
				t.Errorf("status = %d, want %d (body: %s)", w.Code, tc.wantStatus, w.Body.String())
			}
			if called != tc.wantCalled {
				t.Errorf("authorizer called = %v, want %v", called, tc.wantCalled)
			}
			if tc.wantCalled {
				if gotOrgID != tc.orgID {
					t.Errorf("authorizer orgID = %q, want %q", gotOrgID, tc.orgID)
				}
				if tc.userID != "" && gotUserID != tc.userID {
					t.Errorf("authorizer userID = %q, want %q", gotUserID, tc.userID)
				}
			}
		})
	}
}

// TestRequireActiveMembership_TrimsHeader verifies surrounding whitespace in the
// x-user-id header is trimmed before the membership lookup (defense against a
// padded header slipping past an exact-match comparison downstream).
func TestRequireActiveMembership_TrimsHeader(t *testing.T) {
	authz := &fakeAuthorizer{member: true}
	gin.SetMode(gin.TestMode)
	r := gin.New()
	r.POST("/orgs/:id/plan", requireActiveMembership(authz), func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{"ok": true})
	})

	req := httptest.NewRequest(http.MethodPost, "/orgs/org_123/plan", http.NoBody)
	req.Header.Set("x-user-id", "  user_abc  ")
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200 (body: %s)", w.Code, w.Body.String())
	}
	if _, _, gotUserID := authz.snapshot(); gotUserID != "user_abc" {
		t.Errorf("authorizer userID = %q, want %q (trimmed)", gotUserID, "user_abc")
	}
}
