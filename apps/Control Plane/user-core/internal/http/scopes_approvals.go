package http

import (
	"net/http"
	"strings"
)

// WS10-S2: scopes of the approval-grant and approval-policy routes, registered
// through the P-UC1 seam (feature rules run only after the core Space table did
// not match, so they cannot re-scope an existing route). Without a rule an
// unmapped POST/PUT/DELETE would fall through to users:write:self.
//
//	POST   /api/v1/internal/spaces/approval-grants                  -> spaces:issue (gateway-delegated)
//	GET    /api/v1/internal/spaces/{ref}/approval-grants             -> spaces:issue
//	DELETE /api/v1/internal/spaces/{ref}/approval-grants/{rule}      -> spaces:issue
//	GET|PUT /api/v1/internal/spaces/{ref}/approval-policy            -> spaces:issue
//	POST   /api/v1/internal/spaces/tool-gate-view                    -> spaces:tool-gate:view
//	POST   /api/v1/internal/spaces/approval-grants/use               -> spaces:approval-grant:use
//	POST   /api/v1/internal/spaces/approval-continuation-check       -> spaces:approval-continuation:check
//
// spaces:issue is already a delegated scope (a service credential alone never
// proves the acting user). The three execution-core scopes are plain service
// scopes: they act on a run, not on behalf of a person; their credential rows
// ride the Control PR train (PRINCIPAL-TRAIN.md, batch 3).
const (
	spaceToolGateViewScope          = "spaces:tool-gate:view"
	spaceApprovalGrantUseScope      = "spaces:approval-grant:use"
	spaceApprovalContinuationScope  = "spaces:approval-continuation:check"
	spaceInternalPrefixForApprovals = "/api/v1/internal/spaces/"
)

func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		rest, ok := strings.CutPrefix(path, spaceInternalPrefixForApprovals)
		if !ok {
			return "", false
		}
		parts := strings.Split(rest, "/")
		switch {
		case method == http.MethodPost && len(parts) == 1 && parts[0] == "approval-grants":
			return "spaces:issue", true
		case method == http.MethodPost && len(parts) == 1 && parts[0] == "tool-gate-view":
			return spaceToolGateViewScope, true
		case method == http.MethodPost && len(parts) == 2 && parts[0] == "approval-grants" && parts[1] == "use":
			return spaceApprovalGrantUseScope, true
		case method == http.MethodPost && len(parts) == 1 && parts[0] == "approval-continuation-check":
			return spaceApprovalContinuationScope, true
		case len(parts) == 2 && parts[1] == "approval-grants" && method == http.MethodGet,
			len(parts) == 3 && parts[1] == "approval-grants" && method == http.MethodDelete,
			len(parts) == 2 && parts[1] == "approval-policy" && (method == http.MethodGet || method == http.MethodPut):
			return "spaces:issue", true
		}
		return "", false
	})
}
