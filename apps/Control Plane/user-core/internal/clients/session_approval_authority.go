package clients

import (
	"context"
	"fmt"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc/metadata"
)

// WS10-S2 (shared with WS10-S6): Control's read of RunService.ResolveApprovalAuthority,
// the content-free projection Control needs before it records or signs anything about
// one approval. It rides the same narrow Session Core bearer and identity as
// ResolveRunActionAuthority (Control's action-authorizer), so a nil or unconfigured
// client answers an error and never a permissive projection. The response carries no
// approval context, tool input, reason or card text.

// ResolveApprovalAuthority returns the approval's projection. An approval Session Core
// does not resolve answers spaces.ErrApprovalAuthorityNotResolved; any other error is
// an availability failure.
func (c *SessionRunActionAuthorityClient) ResolveApprovalAuthority(ctx context.Context, approvalID, orgID string) (spaces.ApprovalDecideAuthority, error) {
	if c == nil || c.client == nil || strings.TrimSpace(c.bearer) == "" {
		return spaces.ApprovalDecideAuthority{}, fmt.Errorf("the Session Core approval authority client is not configured")
	}
	approvalID = strings.TrimSpace(approvalID)
	orgID = strings.TrimSpace(orgID)
	if approvalID == "" || orgID == "" {
		return spaces.ApprovalDecideAuthority{}, fmt.Errorf("approval and organization are required")
	}
	response, err := c.client.ResolveApprovalAuthority(
		metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+c.bearer),
		&mpv1.ResolveApprovalAuthorityRequest{ApprovalId: approvalID, OrgId: orgID},
	)
	if err != nil {
		return spaces.ApprovalDecideAuthority{}, fmt.Errorf("resolve Session Core approval authority: %w", err)
	}
	return approvalAuthorityFromResponse(response)
}

func approvalAuthorityFromResponse(response *mpv1.ResolveApprovalAuthorityResponse) (spaces.ApprovalDecideAuthority, error) {
	if response == nil || !response.GetResolved() {
		return spaces.ApprovalDecideAuthority{}, spaces.ErrApprovalAuthorityNotResolved
	}
	authority := spaces.ApprovalDecideAuthority{
		ApprovalID:         strings.TrimSpace(response.GetApprovalId()),
		OrgID:              strings.TrimSpace(response.GetOrgId()),
		SpaceRef:           strings.TrimSpace(response.GetSpaceId()),
		RunID:              strings.TrimSpace(response.GetRunId()),
		RequesterSubjectID: strings.TrimSpace(response.GetRequesterSubjectId()),
		AddresseeKind:      strings.TrimSpace(response.GetAddresseeKind()),
		AddresseeSubjectID: strings.TrimSpace(response.GetAddresseeSubjectId()),
		TurnOrigin:         strings.TrimSpace(response.GetTurnOrigin()),
		State:              strings.TrimSpace(response.GetApprovalStatus()),
	}
	// A resolved answer with a missing identifier is a contract mismatch, not an
	// absent approval: it is an availability failure, never a 404.
	if authority.ApprovalID == "" || authority.OrgID == "" || authority.SpaceRef == "" ||
		authority.RunID == "" || authority.RequesterSubjectID == "" || authority.State == "" {
		return spaces.ApprovalDecideAuthority{}, fmt.Errorf("invalid Session Core approval authority: incomplete projection")
	}
	return authority, nil
}
