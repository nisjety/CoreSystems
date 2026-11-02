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

	// ScopeAgent is the only scope an api_key principal uses here (INNH-10):
	// it lets a key acquire an allow-list session grant and revoke its own.
	ScopeAgent = "browser:agent"

	// GrantKindOriginExtension is the AcquireGrant grant_kind of an origin
	// extension (E3.2-e1).
	GrantKindOriginExtension = "origin_extension"

	// PrincipalUser, PrincipalService and PrincipalAPIKey are the authctx
	// principal types this broker authorizes.
	PrincipalUser    = "user"
	PrincipalService = "service"
	PrincipalAPIKey  = authctx.PrincipalTypeAPIKey
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
		// INNH-10: the shape is verified here; Authorize and the server decide
		// what a key may do (nothing while BROWSER_BROKER_KEY_GRANTS_ENABLED is
		// off).
		AcceptAPIKey: true,
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
		// E3.2-e1: an origin extension records a person's consent; no workload
		// issues one on its own authority (the handler refuses it too).
		if req, ok := request.(*mpv1.AcquireGrantRequest); ok && req.GetGrantKind() == GrantKindOriginExtension {
			return errors.New("an origin extension is issued to a signed-in user only")
		}
		return nil
	case PrincipalAPIKey:
		return authorizeAPIKey(principal, method, request)
	default:
		// A principal type this broker was not taught is refused outright. It
		// used to fall through to the scoped-service branch, whose owner filter
		// is the organization-wide empty one (fail open).
		return errors.New("unsupported browser principal type")
	}
}

// authorizeAPIKey is the whole of what an api_key principal may call (INNH-10
// MP-1): AcquireGrant for an ordinary allow-list session grant (no grant_kind,
// so never human_browse; no sensitive scopes and no parent, so never a
// sensitive approval) and RevokeGrant (the handler limits it to the key's own
// grants). Every other method, RenewGrant and DeriveRunGrant included, is
// refused, and both need scope browser:agent. The broker switch
// (BROWSER_BROKER_KEY_GRANTS_ENABLED) is checked by the handlers.
func authorizeAPIKey(principal authctx.Principal, method string, request any) error {
	if principal.KeyID == "" {
		return errors.New("api key principal has no key id")
	}
	if !principal.HasScope(ScopeAgent) {
		return errors.New("required browser scope is missing")
	}
	switch method {
	case "/model_plane.v1.BrowserBroker/AcquireGrant":
		req, ok := request.(*mpv1.AcquireGrantRequest)
		if !ok || req == nil {
			return errors.New("malformed api key grant request")
		}
		return APIKeyAcquireShape(req)
	case "/model_plane.v1.BrowserBroker/RevokeGrant":
		return nil
	default:
		return errors.New("method is not available to an api key")
	}
}

// APIKeyAcquireShape refuses an AcquireGrant request an api_key may never make:
// a grant kind (human_browse), a sensitive approval scope, or a parent grant.
func APIKeyAcquireShape(req *mpv1.AcquireGrantRequest) error {
	if req.GetGrantKind() != "" {
		return errors.New("an api key acquires an ordinary session grant only")
	}
	if len(req.GetAllowedActions()) != 0 || len(req.GetAllowedFrameIds()) != 0 || len(req.GetAllowedDialogIds()) != 0 ||
		len(req.GetAllowedArtifactIds()) != 0 || req.GetParentGrantId() != "" {
		return errors.New("an api key cannot acquire a sensitive approval")
	}
	if req.GetConsentScope() != "" || len(req.GetConsentDeniedDomains()) != 0 {
		return errors.New("an api key cannot carry a consent")
	}
	return nil
}

// KeyMatches reports whether a grant with keyID is visible to principal under
// the key rule: an api_key principal sees only grants stamped with its own key
// (OwnerFilter alone would show it every grant of its owner). Other principal
// types are not narrowed here; the owner filter applies to them.
func KeyMatches(principal authctx.Principal, keyID string) bool {
	if principal.PrincipalType != PrincipalAPIKey {
		return true
	}
	return principal.KeyID != "" && keyID == principal.KeyID
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
// empty filter is a service's alone. An api_key is pinned to its owning user
// here, and every handler that reads or revokes a grant for it also applies
// KeyMatches, so it reaches its own key's grants only. Every other principal
// type gets a filter that matches no grant. It used to get "" for anything that
// was not a user, i.e. any owner in the org: fail open.
func OwnerFilter(principal authctx.Principal) string {
	switch principal.PrincipalType {
	case PrincipalUser:
		return principal.ActorID
	case PrincipalAPIKey:
		if principal.ActorID == "" {
			return noOwner
		}
		return principal.ActorID
	case PrincipalService:
		return ""
	default:
		return noOwner
	}
}
