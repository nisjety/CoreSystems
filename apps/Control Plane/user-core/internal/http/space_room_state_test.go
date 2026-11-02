package http

import (
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS04-S4 / WS04-S7: the service scope of every room writer and archive
// route. These are feature rules registered through the P-UC1 seam, so an
// unmapped POST would otherwise fall through to users:write:self.
func TestRoomWriterAndArchiveScopes(t *testing.T) {
	const prefix = "/api/v1/internal/spaces/space-1"
	cases := []struct {
		name, method, path, want string
	}{
		{"add member", http.MethodPost, prefix + "/members", "spaces:membership:act"},
		{"change role", http.MethodPatch, prefix + "/members/user-2", "spaces:membership:act"},
		{"remove or leave", http.MethodDelete, prefix + "/members/user-2", "spaces:membership:act"},
		{"any other method on members is still the writer scope", http.MethodPut, prefix + "/members", "spaces:membership:act"},
		{"audience candidates read", http.MethodGet, prefix + "/audience-candidates", "spaces:membership:read"},
		{"archive", http.MethodPost, prefix + "/archive", "spaces:membership:act"},
		{"unarchive", http.MethodPost, prefix + "/unarchive", "spaces:membership:act"},
		{"readable membership read keeps the resolve scope", http.MethodGet, prefix + "/membership?readable=1", "spaces:resolve"},
		{"plain membership read keeps the resolve scope", http.MethodGet, prefix + "/membership", "spaces:resolve"},
		{"declarative replace keeps its own scope", http.MethodPut, prefix + "/memberships", "spaces:membership:write"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			// serviceScopeForRequest works on the URL path; the query string is
			// not part of it, so build the request the way the router sees it.
			request := httptest.NewRequest(tc.method, tc.path, nil)
			got := serviceScopeForRequest(request)
			if got != tc.want {
				t.Fatalf("%s %s => %q, want %q", tc.method, tc.path, got, tc.want)
			}
			if got == "users:write:self" || got == "users:read:self" {
				t.Fatalf("%s %s fell through to a user self scope", tc.method, tc.path)
			}
		})
	}
}

// The writer scope is a delegated scope (a service credential alone never
// proves the acting user); the audience read is a plain service scope.
func TestRoomWriterScopeIsDelegatedAndTheAudienceReadIsNot(t *testing.T) {
	if !serviceScopeRequiresVerifiedDelegation("spaces:membership:act") {
		t.Fatal("spaces:membership:act must require a verified delegation")
	}
	if serviceScopeRequiresVerifiedDelegation("spaces:membership:read") {
		t.Fatal("spaces:membership:read is a service read and must not require a delegation")
	}
	if serviceScopeRequiresVerifiedDelegation("spaces:membership:write") {
		t.Fatal("the declarative replace scope stays service-only")
	}
}

// refuseArchivedWrite only ever acts on a no-current-membership resolution.
// Any other outcome, including a clean resolution and an outage, leaves the
// handler's own fail-closed path untouched and writes nothing.
func TestRefuseArchivedWriteIgnoresEveryOtherResolutionOutcome(t *testing.T) {
	gin.SetMode(gin.TestMode)
	for _, resolveErr := range []error{nil, errors.New("database down"), spaces.AuthorityUnavailableDenial("x")} {
		w := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(w)
		c.Request = httptest.NewRequest(http.MethodPost, "/x", nil)
		if (&Server{}).refuseArchivedWrite(c, resolveErr, "space-1") {
			t.Fatalf("refuseArchivedWrite acted on %v", resolveErr)
		}
		if w.Body.Len() != 0 {
			t.Fatalf("refuseArchivedWrite wrote a body for %v: %s", resolveErr, w.Body.String())
		}
	}
}
