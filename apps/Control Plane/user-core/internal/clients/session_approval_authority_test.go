package clients

import (
	"context"
	"errors"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// WS10-S2 (shared with WS10-S6): the Control projection of
// RunService.ResolveApprovalAuthority. The wire call needs a live Session Core;
// these cases pin the mapping and the refusals that come before any call.

func resolvedApprovalResponse() *mpv1.ResolveApprovalAuthorityResponse {
	return &mpv1.ResolveApprovalAuthorityResponse{
		Resolved: true, ApprovalId: "approval-1", RunId: "run-1", OrgId: "org-1",
		RequesterSubjectId: "user-1", SpaceId: "space-room", ApprovalStatus: "granted",
		ApprovalKind: "tool_call", AddresseeKind: "requester", TurnOrigin: "direct",
	}
}

func TestApprovalAuthorityFromResponseMapsTheProjection(t *testing.T) {
	authority, err := approvalAuthorityFromResponse(resolvedApprovalResponse())
	if err != nil {
		t.Fatalf("map resolved projection: %v", err)
	}
	if authority.ApprovalID != "approval-1" || authority.RunID != "run-1" || authority.OrgID != "org-1" ||
		authority.RequesterSubjectID != "user-1" || authority.SpaceRef != "space-room" ||
		authority.State != "granted" || authority.AddresseeKind != "requester" || authority.TurnOrigin != "direct" {
		t.Fatalf("mapped projection = %+v", authority)
	}
}

func TestApprovalAuthorityFromResponseUnresolvedIsTheTypedSentinel(t *testing.T) {
	if _, err := approvalAuthorityFromResponse(nil); !errors.Is(err, spaces.ErrApprovalAuthorityNotResolved) {
		t.Fatalf("nil response: err = %v, want ErrApprovalAuthorityNotResolved", err)
	}
	if _, err := approvalAuthorityFromResponse(&mpv1.ResolveApprovalAuthorityResponse{}); !errors.Is(err, spaces.ErrApprovalAuthorityNotResolved) {
		t.Fatalf("unresolved response: err = %v, want ErrApprovalAuthorityNotResolved", err)
	}
}

func TestApprovalAuthorityFromResponseIncompleteIsAnAvailabilityErrorNotA404(t *testing.T) {
	response := resolvedApprovalResponse()
	response.RequesterSubjectId = ""
	_, err := approvalAuthorityFromResponse(response)
	if err == nil || errors.Is(err, spaces.ErrApprovalAuthorityNotResolved) {
		t.Fatalf("incomplete resolved projection: err = %v, want an availability error", err)
	}
}

func TestResolveApprovalAuthorityRefusesWhenNotConfiguredOrNotAsked(t *testing.T) {
	var nilClient *SessionRunActionAuthorityClient
	if _, err := nilClient.ResolveApprovalAuthority(context.Background(), "approval-1", "org-1"); err == nil {
		t.Fatal("a nil client resolved an approval")
	}
	if _, err := (&SessionRunActionAuthorityClient{}).ResolveApprovalAuthority(context.Background(), "approval-1", "org-1"); err == nil {
		t.Fatal("an unconfigured client resolved an approval")
	}
	configured := &SessionRunActionAuthorityClient{client: mpv1.NewRunServiceClient(nil), bearer: "bearer-for-tests"}
	if _, err := configured.ResolveApprovalAuthority(context.Background(), " ", "org-1"); err == nil {
		t.Fatal("a blank approval id reached Session Core")
	}
	if _, err := configured.ResolveApprovalAuthority(context.Background(), "approval-1", " "); err == nil {
		t.Fatal("a blank organisation reached Session Core")
	}
}
