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

	// PrincipalUser and PrincipalService are the authctx principal types this
	// broker authorizes.
	PrincipalUser    = "user"
	PrincipalService = "service"
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
	var requestOrg string
	switch typed := request.(type) {
	case *mpv1.AcquireGrantRequest:
		requestOrg = typed.GetOrgId()
	case *mpv1.DeriveRunGrantRequest:
		requestOrg = typed.GetOrgId()
	case *mpv1.RenewGrantRequest:
		requestOrg = typed.GetOrgId()
	}
	if orgID := strings.TrimSpace(requestOrg); orgID != "" && orgID != principal.OrganizationID {
		return errors.New("request organization does not match token")
	}
	required := ScopeRead
	switch method {
	case "/model_plane.v1.BrowserBroker/AcquireGrant",
		"/model_plane.v1.BrowserBroker/RevokeGrant",
		"/model_plane.v1.BrowserBroker/DeriveRunGrant",
		"/model_plane.v1.BrowserBroker/RenewGrant":
		// RenewGrant (WS8.C) is write-scoped for a service, and no service holds
		// browser:write, so only a user can renew (the handler also refuses a
		// non-user principal).
		required = ScopeWrite
	case "/model_plane.v1.BrowserBroker/ValidateGrant", "/model_plane.v1.BrowserBroker/Health":
	default:
		return errors.New("unknown browser-broker method")
	}
	switch principal.PrincipalType {
	case PrincipalUser:
		return nil
	case PrincipalService:
		if !principal.HasScope(required) {
			return errors.New("required browser scope is missing")
		}
		return nil
	default:
		// A principal type this broker was not taught is refused outright. It
		// used to fall through to the scoped-service branch, whose owner filter
		// is the organization-wide empty one (fail open).
		return errors.New("unsupported browser principal type")
	}
}

// Principal returns the cryptographically verified caller or fails closed.
func Principal(ctx context.Context) (authctx.Principal, error) {
	principal, ok := authctx.PrincipalFromContext(ctx)
	if !ok {
		return authctx.Principal{}, status.Error(codes.Unauthenticated, "verified identity is required")
	}
	return principal, nil
}

// noOwner is the owner filter of a principal type the broker does not know. It
// can never equal a stored owner: the durable store answers NotFound for a NUL
// byte without a query, and no verified identity carries one.
const noOwner = "\x00unknown-principal"

// OwnerFilter pins users to their own records. A service with an explicit
// method scope may access any owner inside its verified organization: the
// empty filter is a service's alone. Every other principal type gets a filter
// that matches no grant. It used to get "" for anything that was not a user,
// i.e. any owner in the org: fail open.
func OwnerFilter(principal authctx.Principal) string {
	switch principal.PrincipalType {
	case PrincipalUser:
		return principal.ActorID
	case PrincipalService:
		return ""
	default:
		return noOwner
	}
}
