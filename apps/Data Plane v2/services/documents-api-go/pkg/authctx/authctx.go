// Package authctx verifies Control Plane-issued, audience-scoped JWTs and
// derives tenant/user identity from their signed claims.
//
// Two modes:
//
//   - **Observe** (local development only, `AUTHCTX_ENFORCE=0` together with
//     `ALLOW_INSECURE_DEV_DEFAULTS=1`): the middleware
//     decodes the `Authorization: Bearer <jwt>` header if present, parses
//     unverified claims, stuffs them into the request context, and logs
//     a structured event when the JWT org_id disagrees with the
//     `X-Org-ID` header. Requests without a JWT pass straight through —
//     the existing `handler.OrgIDMiddleware` still extracts the org id
//     from the header.
//
//   - **Enforce** (the default): every request to a guarded route
//     must carry a JWT that verifies against auth-core's JWKS, with the
//     correct audience, a non-empty `org_id` claim, and a non-expired
//     timestamp. Missing or invalid token → 401; verified JWT whose
//     `org_id` disagrees with `X-Org-ID` → 403. Header-only callers stop
//     working — by design, that's how the multi-tenant trust gap closes.
//
// Verification is implemented with RS256, issuer, audience, expiry, and
// JWKS/static-key checks. Production startup must call Validate before
// serving so missing verification material fails the process closed.
//
// Usage:
//
//	r.Route("/v1/documents", func(r chi.Router) {
//	    r.Use(authctx.Middleware(authctx.Config{
//	        Audience:        "data-plane",
//	        JWKSURL:         "http://auth-core:3011/api/convex-auth/jwks",
//	        ExpectedIssuer:  "http://auth-core:3011/api/convex-auth",
//	    }))
//	    // ... existing routes
//	})
//
// And in handlers:
//
//	claims, ok := authctx.FromContext(r.Context())
//	if !ok || claims.OrgID == "" {
//	    // fallback to legacy header path while observe mode is on
//	}
package authctx

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"slices"
	"strings"
	"time"

	"github.com/rs/zerolog/log"
)

// Claims is the canonical shape every Data Plane service expects to read
// from a verified auth-core JWT. Mirrors the issuance side in
// `apps/Control Plane/auth-core/src/auth/convex-token.service.ts`
// (`issuePlaneToken`).
type Claims struct {
	UserID        string   `json:"user_id"`
	ServiceID     string   `json:"service_id"`
	PrincipalType string   `json:"principal_type"`
	OrgID         string   `json:"org_id"`
	Email         string   `json:"email,omitempty"`
	Scopes        []string `json:"scopes,omitempty"`
	Issuer        string   `json:"iss"`
	Audience      string   `json:"aud"`
	Subject       string   `json:"sub"`
	IssuedAt      int64    `json:"iat"`
	NotBefore     int64    `json:"nbf,omitempty"`
	ExpiresAt     int64    `json:"exp"`
	ZDR           bool     `json:"zdr"`
	// ZDRPresent distinguishes an issuer-signed false posture from a missing or
	// malformed claim. The verifier requires presence before setting Verified.
	ZDRPresent bool `json:"-"`
	// Verified is true when the signature + standard claims have been
	// checked against the JWKS. Observe-mode middleware reads + parses
	// JWTs without verifying — never trust an unverified Claims struct
	// for authorisation decisions.
	Verified bool `json:"-"`
}

func (c *Claims) IsService() bool {
	return c != nil && c.Verified && c.PrincipalType == "service"
}

func (c *Claims) PrincipalID() string {
	if c == nil || !c.Verified {
		return ""
	}
	switch c.PrincipalType {
	case "user":
		return c.UserID
	case "service":
		return c.ServiceID
	default:
		return ""
	}
}

// HasScope reports whether the verified claims grant the supplied scope.
// Returns false on unverified claims even if the scope is present (the
// caller cannot trust unverified data for authorisation decisions).
func (c *Claims) HasScope(scope string) bool {
	if c == nil || !c.Verified {
		return false
	}
	return slices.Contains(c.Scopes, scope)
}

// IsExpired reports whether `exp` is in the past. Defensive against
// clock-skew with a 30s grace window — matches the velion mint-side
// `REFRESH_SAFETY_MS` so the two sides treat the boundary consistently.
func (c *Claims) IsExpired(now time.Time) bool {
	if c == nil || c.ExpiresAt == 0 {
		return true
	}
	return now.Unix() > c.ExpiresAt+30
}

type contextKey struct{}
type authorizationContextKey struct{}

// Config controls Middleware behaviour. Audience + JWKSURL +
// ExpectedIssuer must be set; everything else has sane defaults.
type Config struct {
	// Audience is the value of the `aud` claim that this service's
	// JWTs must carry (e.g. "data-plane"). Tokens minted for a
	// different audience are rejected even if signature is valid.
	Audience string

	// JWKSURL is the auth-core endpoint that publishes the verification
	// public key set. Default: http://auth-core:3011/api/convex-auth/jwks
	JWKSURL string

	// ExpectedIssuer is the `iss` claim value. Default:
	// http://auth-core:3011/api/convex-auth.
	ExpectedIssuer string

	// Enforce, when true, switches the middleware into enforce mode.
	// Normally controlled via the AUTHCTX_ENFORCE env var; the explicit
	// field exists so tests can override without touching the
	// environment.
	Enforce *bool
}

func (c *Config) resolveEnforce() bool {
	if c.Enforce != nil {
		return *c.Enforce
	}
	switch strings.ToLower(strings.TrimSpace(os.Getenv("AUTHCTX_ENFORCE"))) {
	case "0", "false", "no", "off":
		return false
	default:
		return true
	}
}

// Validate checks the production auth posture before the HTTP server starts.
// Observe mode is available only behind the explicit insecure-development flag;
// enforce mode requires a usable static key or configured JWKS endpoint.
func Validate(cfg Config) error {
	if !cfg.resolveEnforce() {
		if os.Getenv("ALLOW_INSECURE_DEV_DEFAULTS") != "1" {
			return errors.New("authctx: disabling JWT enforcement requires ALLOW_INSECURE_DEV_DEFAULTS=1")
		}
		return nil
	}
	_, err := newVerifier(cfg)
	return err
}

// Middleware returns a chi-compatible middleware. In observe mode it
// decodes the JWT payload (no signature check) and stuffs Claims into
// the request context. In enforce mode it verifies the RS256 signature,
// audience, issuer, and expiry against auth-core's public key (mirroring
// the retrieval-engine Rust verifier) and 401s any request without a
// valid token; a verified org that disagrees with X-Org-ID is 403'd.
func Middleware(cfg Config) func(http.Handler) http.Handler {
	enforce := cfg.resolveEnforce()
	audience := strings.TrimSpace(cfg.Audience)
	logger := log.With().Str("component", "authctx").Str("audience", audience).Logger()

	// Build the verifier once at wiring time. If enforce is on but no
	// verification key is available, keep the fail-closed kill switch:
	// return a middleware that 503s every request rather than silently
	// trusting headers.
	var v *verifier
	if enforce {
		built, err := newVerifier(cfg)
		if err != nil {
			logger.Error().Err(err).Msg("AUTHCTX_ENFORCE=1 but no verification key is configured; returning 503")
			return func(next http.Handler) http.Handler {
				return http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
					http.Error(w, `{"error":"authctx enforce misconfigured"}`, http.StatusServiceUnavailable)
				})
			}
		}
		v = built
	}

	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			token := extractBearer(r)

			if enforce {
				if token == "" {
					http.Error(w, `{"error":"missing bearer token"}`, http.StatusUnauthorized)
					return
				}
				claims, err := v.Verify(token)
				if err != nil {
					logger.Warn().Err(err).Msg("authctx enforce: token verification failed")
					http.Error(w, `{"error":"invalid token"}`, http.StatusUnauthorized)
					return
				}
				// Verified org is authoritative; reject a mismatched header
				// so a leaked JWT can't be paired with another tenant's id.
				if headerOrg := strings.TrimSpace(r.Header.Get("X-Org-ID")); headerOrg != "" && headerOrg != claims.OrgID {
					logger.Warn().
						Str("header_org_id", headerOrg).
						Str("jwt_org_id", claims.OrgID).
						Str("user_id", claims.UserID).
						Msg("authctx enforce: X-Org-ID disagrees with verified JWT org_id; rejecting")
					http.Error(w, `{"error":"org mismatch"}`, http.StatusForbidden)
					return
				}
				// Re-stamp X-Org-ID from the verified claim so downstream
				// OrgIDMiddleware + handlers scope to the trusted tenant.
				r.Header.Set("X-Org-ID", claims.OrgID)
				ctx := IntoContext(r.Context(), claims)
				ctx = context.WithValue(ctx, authorizationContextKey{}, "Bearer "+token)
				next.ServeHTTP(w, r.WithContext(ctx))
				return
			}

			if token == "" {
				// Observe mode: header-only callers continue working.
				next.ServeHTTP(w, r)
				return
			}

			claims, err := decodeUnverified(token)
			if err != nil {
				logger.Warn().Err(err).Msg("failed to decode bearer JWT in observe mode; forwarding without claims")
				next.ServeHTTP(w, r)
				return
			}

			// Soft-validate audience even in observe mode so we catch
			// audience mix-ups in telemetry well before they become
			// security incidents in enforce mode.
			if audience != "" && claims.Audience != audience {
				logger.Warn().
					Str("expected_audience", audience).
					Str("got_audience", claims.Audience).
					Str("user_id", claims.UserID).
					Str("org_id", claims.OrgID).
					Msg("authctx observe: JWT audience mismatch")
			}

			// Compare against legacy header for drift detection. If
			// they disagree, the calling code is sending one tenant's
			// JWT with another tenant's header — log loudly.
			if headerOrg := r.Header.Get("X-Org-ID"); headerOrg != "" && headerOrg != claims.OrgID {
				logger.Warn().
					Str("header_org_id", headerOrg).
					Str("jwt_org_id", claims.OrgID).
					Str("user_id", claims.UserID).
					Msg("authctx observe: X-Org-ID disagrees with JWT org_id; trusted X-Org-ID for now")
			}

			ctx := IntoContext(r.Context(), claims)
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}
}

// RequireServiceScope preserves ordinary verified user access while ensuring a
// service-principal token cannot be replayed across unrelated Data services.
func RequireServiceScope(scope string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			claims, ok := FromContext(r.Context())
			if !ok || claims.PrincipalID() == "" {
				http.Error(w, `{"error":"invalid principal"}`, http.StatusUnauthorized)
				return
			}
			if claims.PrincipalType == "user" {
				next.ServeHTTP(w, r)
				return
			}
			if claims.IsService() && claims.HasScope(scope) {
				next.ServeHTTP(w, r)
				return
			}
			http.Error(w, `{"error":"insufficient service scope"}`, http.StatusForbidden)
		})
	}
}

// IntoContext stores Claims in the request context. Exported so tests
// (and future helpers) can bypass the middleware when building fake
// request contexts.
func IntoContext(ctx context.Context, claims *Claims) context.Context {
	return context.WithValue(ctx, contextKey{}, claims)
}

// FromContext returns the Claims previously stored by Middleware. The
// `ok` return is false when no claims are present (e.g. an unguarded
// route, or observe mode with a header-only caller).
func FromContext(ctx context.Context) (*Claims, bool) {
	v, ok := ctx.Value(contextKey{}).(*Claims)
	return v, ok && v != nil
}

// AuthorizationHeader returns the original bearer only after Middleware has
// cryptographically verified it. Callers may forward this proof to an
// authorization authority, but must never log or persist it.
func AuthorizationHeader(ctx context.Context) (string, bool) {
	value, ok := ctx.Value(authorizationContextKey{}).(string)
	return value, ok && strings.HasPrefix(value, "Bearer ") && len(value) > len("Bearer ")
}

// extractBearer pulls the JWT out of `Authorization: Bearer <token>`.
// Returns the empty string when the header is missing or malformed.
// Tolerates lowercase `bearer` — RFC 6750 makes the scheme
// case-insensitive even though most clients capitalise.
func extractBearer(r *http.Request) string {
	header := r.Header.Get("Authorization")
	if header == "" {
		return ""
	}
	const prefix = "Bearer "
	if len(header) < len(prefix) || !strings.EqualFold(header[:len(prefix)], prefix) {
		return ""
	}
	return strings.TrimSpace(header[len(prefix):])
}

// decodeUnverified parses the JWT payload WITHOUT checking the
// signature. Used only in observe mode for telemetry. The returned
// Claims has `Verified=false` — never use it for authorisation.
func decodeUnverified(token string) (*Claims, error) {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return nil, errors.New("authctx: malformed JWT (expected 3 dot-separated segments)")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		// Some encoders pad — try standard URL encoding too.
		payload, err = base64.URLEncoding.DecodeString(parts[1])
		if err != nil {
			return nil, errors.New("authctx: failed to base64url-decode JWT payload")
		}
	}
	claims := &Claims{}
	if err := json.Unmarshal(payload, claims); err != nil {
		return nil, errors.New("authctx: failed to JSON-decode JWT payload")
	}
	claims.Verified = false
	return claims, nil
}
