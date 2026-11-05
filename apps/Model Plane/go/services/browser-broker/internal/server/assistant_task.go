package server

import (
	"context"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/sitepolicy"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

const (
	grantKindAssistantTask = "assistant_task"
	assistantTaskGrantTTL  = 5 * time.Minute
)

func (s *Server) validateAssistantTaskGrant(req *AcquireGrantRequest, principal authctx.Principal, scopeURL string) error {
	if !s.options.AssistantTaskGrantsEnabled || principal.PrincipalType != authz.PrincipalUser || principal.ActorID == "" {
		return status.Error(codes.PermissionDenied, "assistant task grants require an enabled, signed-in user session")
	}
	if scopeURL != "browser://cloud" || len(req.GetAllowedDomains()) != 1 || req.GetReadOnly() {
		return status.Error(codes.InvalidArgument, "an assistant task grant requires one cloud browser host")
	}
	if len(req.GetAllowedActions()) != 0 || len(req.GetAllowedFrameIds()) != 0 || len(req.GetAllowedDialogIds()) != 0 ||
		len(req.GetAllowedArtifactIds()) != 0 || req.GetParentGrantId() != "" {
		return status.Error(codes.InvalidArgument, "an assistant task grant cannot carry sensitive scopes or a parent")
	}
	if req.GetBudgetMicroUsd() != 0 || req.GetBudgetMaxSteps() != 0 || req.GetBudgetMaxRuntimeS() != 0 ||
		req.GetBudgetSource() != "" || req.GetBudgetMintedAtUnix() != 0 {
		return status.Error(codes.InvalidArgument, "an assistant task grant cannot carry a budget claim")
	}
	return nil
}

// A submitted task can supply consent for the default ask case. Org-core must
// still affirm the act ceiling, and an explicit member or org decision wins.
func (s *Server) checkAssistantTaskSitePolicy(ctx context.Context, principal authctx.Principal, domains []string) error {
	resolution, err := s.resolveSites(ctx, principal, domains)
	if err != nil {
		recordSiteVerdict(ctx, "policy_unavailable")
		return status.Error(codes.Unavailable, sitepolicy.ErrUnavailable.Error())
	}
	if len(resolution.Decisions) == 1 {
		decision := resolution.Decisions[0]
		if decision.Host == domains[0] && (decision.Effective == sitepolicy.EffectiveAct ||
			(decision.Effective == sitepolicy.EffectiveAsk && decision.Cap == "act" && decision.Source == "default")) {
			recordSiteVerdict(ctx, "assistant_task_allowed")
			return nil
		}
	}
	recordSiteVerdict(ctx, "denied")
	return siteDeniedError(domains)
}
