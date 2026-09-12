// Package authz defines sandbox-manager's authenticated authorization boundary.
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
	Audience   = "sandbox-manager"
	ScopeRead  = "sandbox:read"
	ScopeWrite = "sandbox:write"
)

func NewVerifierFromEnv() (*authctx.Verifier, error) {
	if configured := strings.TrimSpace(os.Getenv("SANDBOX_MANAGER_AUTH_AUDIENCE")); configured != "" && configured != Audience {
		return nil, errors.New("sandbox-manager auth audience must be sandbox-manager")
	}
	return authctx.NewVerifier(authctx.Config{
		Audiences: []string{Audience},
		Issuer:    os.Getenv("AUTH_CORE_ISSUER"),
		JWKSURL:   os.Getenv("AUTH_CORE_JWKS_URL"),
	})
}

func UnaryInterceptor(verifier *authctx.Verifier) grpc.UnaryServerInterceptor {
	authenticate := verifier.UnaryServerInterceptor(Authorize)
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
		if strings.HasPrefix(info.FullMethod, "/grpc.health.v1.Health/") {
			return handler(ctx, req)
		}
		return authenticate(ctx, req, info, handler)
	}
}

func Authorize(principal authctx.Principal, method string, request any) error {
	if acquire, ok := request.(*mpv1.AcquireLeaseRequest); ok {
		orgID := strings.TrimSpace(acquire.GetOrgId())
		if orgID != "" && orgID != principal.OrganizationID {
			return errors.New("request organization does not match token")
		}
	}
	required := ScopeRead
	switch method {
	// A confirmed pre-existing bug, found and fixed here (2026-09-12, S3.3
	// step 3.5.C design doc §8 item 3.5.C): ActivateLease was never listed
	// in this switch, so any call to it through this real interceptor was
	// refused with "unknown sandbox-manager method" for any principal,
	// dormant only because nothing had ever called it that way (every
	// existing test bypasses the interceptor by calling the Server method
	// directly). 3.5.C gives it its first real caller.
	case "/model_plane.v1.SandboxManager/AcquireLease",
		"/model_plane.v1.SandboxManager/ReleaseLease",
		"/model_plane.v1.SandboxManager/SnapshotSandbox",
		"/model_plane.v1.SandboxManager/ActivateLease":
		required = ScopeWrite
	case "/model_plane.v1.SandboxManager/Health",
		"/model_plane.v1.SandboxManager/GetWorkspaceManifest":
	default:
		return errors.New("unknown sandbox-manager method")
	}
	if principal.PrincipalType == "user" {
		return nil
	}
	if !principal.HasScope(required) {
		return errors.New("required sandbox scope is missing")
	}
	return nil
}

func Principal(ctx context.Context) (authctx.Principal, error) {
	principal, ok := authctx.PrincipalFromContext(ctx)
	if !ok {
		return authctx.Principal{}, status.Error(codes.Unauthenticated, "verified identity is required")
	}
	return principal, nil
}

func OwnerFilter(principal authctx.Principal) string {
	if principal.PrincipalType == "user" {
		return principal.ActorID
	}
	return ""
}
