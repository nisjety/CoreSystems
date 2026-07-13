package http

import (
	"context"
	"errors"
	stdhttp "net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"
)

func membershipProjectionContext() (*gin.Context, *httptest.ResponseRecorder) {
	recorder := httptest.NewRecorder()
	ctx, _ := gin.CreateTestContext(recorder)
	ctx.Request = httptest.NewRequest(stdhttp.MethodGet, "/api/v1/me/session-context", nil)
	return ctx, recorder
}

func TestAuthoritativeMembershipDenialRemovesExactLocalProjection(t *testing.T) {
	var removedUserID string
	var removedOrgID string
	server := &Server{
		removeMembershipProjection: func(_ context.Context, userID, orgID string) error {
			removedUserID = userID
			removedOrgID = orgID
			return nil
		},
	}
	ctx, recorder := membershipProjectionContext()

	denied, continueRequest := server.handleMembershipResolutionError(
		ctx,
		" user-1 ",
		" org-1 ",
		ErrMembershipNotFound,
	)

	if !denied || !continueRequest {
		t.Fatalf("denied/continue = %v/%v, want true/true", denied, continueRequest)
	}
	if removedUserID != "user-1" || removedOrgID != "org-1" {
		t.Fatalf("removed membership = (%q, %q), want exact requested (user-1, org-1)", removedUserID, removedOrgID)
	}
	if recorder.Code != stdhttp.StatusOK || recorder.Body.Len() != 0 {
		t.Fatalf("unexpected response before session context: status/body = %d/%s", recorder.Code, recorder.Body.String())
	}
}

func TestMembershipAuthorityOutageDoesNotMutateLocalProjection(t *testing.T) {
	removeCalls := 0
	server := &Server{
		removeMembershipProjection: func(context.Context, string, string) error {
			removeCalls++
			return nil
		},
	}
	ctx, recorder := membershipProjectionContext()
	authorityErr := errors.New("auth core unavailable")

	denied, continueRequest := server.handleMembershipResolutionError(ctx, "user-1", "org-1", authorityErr)

	if denied || continueRequest {
		t.Fatalf("denied/continue = %v/%v, want false/false", denied, continueRequest)
	}
	if removeCalls != 0 {
		t.Fatalf("remove calls = %d, want 0 during authority outage", removeCalls)
	}
	if recorder.Code != stdhttp.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503; body=%s", recorder.Code, recorder.Body.String())
	}
}

func TestMembershipProjectionRemovalFailureFailsRequestVisibly(t *testing.T) {
	databaseErr := errors.New("database unavailable")
	server := &Server{
		removeMembershipProjection: func(context.Context, string, string) error {
			return databaseErr
		},
	}
	ctx, recorder := membershipProjectionContext()

	denied, continueRequest := server.handleMembershipResolutionError(
		ctx,
		"user-1",
		"org-1",
		ErrMembershipNotFound,
	)

	if !denied || continueRequest {
		t.Fatalf("denied/continue = %v/%v, want true/false", denied, continueRequest)
	}
	if recorder.Code != stdhttp.StatusInternalServerError {
		t.Fatalf("status = %d, want 500; body=%s", recorder.Code, recorder.Body.String())
	}
	if recorder.Body.String() != `{"error":"failed to revoke stale organization membership"}` {
		t.Fatalf("body = %s, want visible revocation failure", recorder.Body.String())
	}
}

func TestMembershipDenialWithoutExactRequestedOrganizationDoesNotMutate(t *testing.T) {
	removeCalls := 0
	server := &Server{
		removeMembershipProjection: func(context.Context, string, string) error {
			removeCalls++
			return nil
		},
	}
	ctx, recorder := membershipProjectionContext()

	denied, continueRequest := server.handleMembershipResolutionError(ctx, "user-1", " ", ErrMembershipNotFound)

	if !denied || !continueRequest {
		t.Fatalf("denied/continue = %v/%v, want true/true", denied, continueRequest)
	}
	if removeCalls != 0 {
		t.Fatalf("remove calls = %d, want 0 without an exact requested organization", removeCalls)
	}
	if recorder.Body.Len() != 0 {
		t.Fatalf("unexpected response body = %s", recorder.Body.String())
	}
}

func TestMembershipDenialFailsClosedWithoutValidatedUserOrProjectionStore(t *testing.T) {
	tests := []struct {
		name   string
		server *Server
		userID string
	}{
		{
			name: "missing user",
			server: &Server{removeMembershipProjection: func(context.Context, string, string) error {
				t.Fatal("projection removal must not receive an empty user")
				return nil
			}},
			userID: " ",
		},
		{name: "missing projection store", server: &Server{}, userID: "user-1"},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			ctx, recorder := membershipProjectionContext()
			denied, continueRequest := test.server.handleMembershipResolutionError(ctx, test.userID, "org-1", ErrMembershipNotFound)
			if !denied || continueRequest {
				t.Fatalf("denied/continue = %v/%v, want true/false", denied, continueRequest)
			}
			if recorder.Code != stdhttp.StatusInternalServerError {
				t.Fatalf("status = %d, want 500; body=%s", recorder.Code, recorder.Body.String())
			}
		})
	}
}
