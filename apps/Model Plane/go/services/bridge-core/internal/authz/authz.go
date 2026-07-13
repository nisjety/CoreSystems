// Package authz defines bridge-core's authenticated HTTP boundary.
package authz

import (
	"context"
	"errors"
	"net/http"
	"os"
	"strings"

	"github.com/triodelab/model-plane/pkg/authctx"
	"google.golang.org/grpc"
)

const (
	Audience   = "bridge-core"
	ScopeRead  = "bridge:read"
	ScopeWrite = "bridge:write"
)

func NewVerifierFromEnv() (*authctx.Verifier, error) {
	if configured := strings.TrimSpace(os.Getenv("BRIDGE_CORE_AUTH_AUDIENCE")); configured != "" && configured != Audience {
		return nil, errors.New("bridge-core auth audience must be bridge-core")
	}
	return authctx.NewVerifier(authctx.Config{
		Audiences: []string{Audience},
		Issuer:    os.Getenv("AUTH_CORE_ISSUER"),
		JWKSURL:   os.Getenv("AUTH_CORE_JWKS_URL"),
	})
}

// Authorize requires explicit scopes for workload identities. User identities
// are subsequently restricted to their own sessions by the server/store.
func Authorize(principal authctx.Principal, request *http.Request) error {
	if orgID := strings.TrimSpace(request.URL.Query().Get("org_id")); orgID != "" && orgID != principal.OrganizationID {
		return errors.New("request organization does not match token")
	}
	if principal.PrincipalType == "user" {
		return nil
	}
	required := ScopeRead
	if request.Method != http.MethodGet && request.Method != http.MethodHead {
		required = ScopeWrite
	}
	if !principal.HasScope(required) {
		return errors.New("required bridge scope is missing")
	}
	return nil
}

func OwnerFilter(principal authctx.Principal) string {
	if principal.PrincipalType == "user" {
		return principal.ActorID
	}
	return ""
}

// UnaryInterceptor makes the otherwise empty gRPC listener fail closed for
// every future method while keeping the standard health service public.
func UnaryInterceptor(verifier *authctx.Verifier) grpc.UnaryServerInterceptor {
	authenticate := verifier.UnaryServerInterceptor(func(_ authctx.Principal, _ string, _ any) error {
		return errors.New("bridge-core exposes no authenticated gRPC application methods")
	})
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
		if strings.HasPrefix(info.FullMethod, "/grpc.health.v1.Health/") {
			return handler(ctx, req)
		}
		return authenticate(ctx, req, info, handler)
	}
}
