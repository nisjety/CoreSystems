package authz

import (
	"context"
	"os"
	"strings"

	"github.com/triodelab/model-plane/pkg/zdrrefusal"
	"google.golang.org/grpc"
)

// RequireRetentionClaimEnv switches on refusing a token that carries no
// retention claim at all. Exact match: only the literal "on" enables it.
const RequireRetentionClaimEnv = "SANDBOX_MANAGER_REQUIRE_RETENTION_CLAIM"

// RequireRetentionClaimFromEnv reads the switch. Anything but "on" is off.
func RequireRetentionClaimFromEnv(getenv func(string) string) bool {
	if getenv == nil {
		getenv = os.Getenv
	}
	return strings.TrimSpace(getenv(RequireRetentionClaimEnv)) == "on"
}

// ZDRGate refuses every sandbox-manager method (Health excepted) for a caller
// whose verified token says zero data retention applies. Leases, snapshots,
// workspace promotion, thread workspaces and process output are all durable
// rows, so no sandbox-manager method is open to a ZDR principal.
//
// It must be chained after UnaryInterceptor: it reads the verified principal
// from the context and never looks at a token itself. The authctx interceptor
// flattens authorizer errors into a plain PermissionDenied, which is why the
// typed refusal lives in a second interceptor rather than in Authorize.
//
// An absent retention claim (a token minted before the claim existed) is
// refused the same way only when SANDBOX_MANAGER_REQUIRE_RETENTION_CLAIM is
// "on"; it stays off until Auth Core's real mint for this audience is shown to
// carry zdr:false for both registered callers.
func ZDRGate() grpc.UnaryServerInterceptor {
	return zdrGate(RequireRetentionClaimFromEnv(os.Getenv))
}

func zdrGate(requireClaim bool) grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
		if strings.HasPrefix(info.FullMethod, "/grpc.health.v1.Health/") {
			return handler(ctx, req)
		}
		principal, err := Principal(ctx)
		if err != nil {
			// Chained after authentication, so a missing principal is a wiring
			// defect: fail closed rather than let an unidentified caller by.
			return nil, err
		}
		if principal.ZeroDataRetention || (requireClaim && !principal.RetentionPolicyPresent) {
			return nil, zdrrefusal.Refuse(ctx, zdrrefusal.DurableMutationForbidden, "")
		}
		return handler(ctx, req)
	}
}
