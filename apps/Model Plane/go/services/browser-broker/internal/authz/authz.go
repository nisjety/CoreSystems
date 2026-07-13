// Package authz defines browser-broker's authenticated authorization boundary.
package authz

import (
	"context"
	"errors"
	"os"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

const (
	// Audience is the only Auth Core audience accepted by browser-broker.
	Audience   = "browser-broker"
	ScopeRead  = "browser:read"
	ScopeWrite = "browser:write"
)

// NewVerifierFromEnv loads trust material eagerly. It rejects attempts to
// configure a generic or different audience.
func NewVerifierFromEnv() (*authctx.Verifier, error) {
	if configured := strings.TrimSpace(os.Getenv("BROWSER_BROKER_AUTH_AUDIENCE")); configured != "" && configured != Audience {
		return nil, errors.New("browser-broker auth audience must be browser-broker")
	}
	return authctx.NewVerifier(authctx.Config{
		Audiences: []string{Audience},
		Issuer:    os.Getenv("AUTH_CORE_ISSUER"),
		JWKSURL:   os.Getenv("AUTH_CORE_JWKS_URL"),
	})
}

// UnaryInterceptor leaves only the standard gRPC health service public.
func UnaryInterceptor(verifier *authctx.Verifier) grpc.UnaryServerInterceptor {
	authenticate := verifier.UnaryServerInterceptor(Authorize)
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
		if strings.HasPrefix(info.FullMethod, "/grpc.health.v1.Health/") {
			return handler(ctx, req)
		}
		return authenticate(ctx, req, info, handler)
	}
}

// Authorize allows users to operate within their own verified identity and
// requires explicit least-privilege scopes from workload identities.
func Authorize(principal authctx.Principal, method string, request any) error {
	if acquire, ok := request.(*mpv1.AcquireGrantRequest); ok {
		orgID := strings.TrimSpace(acquire.GetOrgId())
		if orgID != "" && orgID != principal.OrganizationID {
			return errors.New("request organization does not match token")
		}
	}
	required := ScopeRead
	switch method {
	case "/model_plane.v1.BrowserBroker/AcquireGrant", "/model_plane.v1.BrowserBroker/RevokeGrant":
		required = ScopeWrite
	case "/model_plane.v1.BrowserBroker/ValidateGrant", "/model_plane.v1.BrowserBroker/Health":
	default:
		return errors.New("unknown browser-broker method")
	}
	if principal.PrincipalType == "user" {
		return nil
	}
	if !principal.HasScope(required) {
		return errors.New("required browser scope is missing")
	}
	return nil
}

// Principal returns the cryptographically verified caller or fails closed.
func Principal(ctx context.Context) (authctx.Principal, error) {
	principal, ok := authctx.PrincipalFromContext(ctx)
	if !ok {
		return authctx.Principal{}, status.Error(codes.Unauthenticated, "verified identity is required")
	}
	return principal, nil
}

// OwnerFilter pins users to their own records. A service with an explicit
// method scope may access any owner inside its verified organization.
func OwnerFilter(principal authctx.Principal) string {
	if principal.PrincipalType == "user" {
		return principal.ActorID
	}
	return ""
}
