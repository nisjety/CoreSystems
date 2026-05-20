// Package authctx is the Phase A · A1.2 stub that prepares Data Plane
// services to switch from "trust the X-Org-ID header" to "derive org_id
// from a verified auth-core JWT".
//
// Two modes:
//
//   - **Observe** (default, `AUTHCTX_ENFORCE` unset or 0): the middleware
//     decodes the `Authorization: Bearer <jwt>` header if present, parses
//     unverified claims, stuffs them into the request context, and logs
//     a structured event when the JWT org_id disagrees with the
//     `X-Org-ID` header. Requests without a JWT pass straight through —
//     the existing `handler.OrgIDMiddleware` still extracts the org id
//     from the header.
//
//   - **Enforce** (`AUTHCTX_ENFORCE=1`): every request to a guarded route
//     must carry a JWT that verifies against auth-core's JWKS, with the
//     correct audience, a non-empty `org_id` claim, and a non-expired
//     timestamp. Missing or invalid token → 401; verified JWT whose
//     `org_id` disagrees with `X-Org-ID` → 403. Header-only callers stop
//     working — by design, that's how the multi-tenant trust gap closes.
//
// The Verify path is intentionally stubbed in this commit (returns
// ErrNotImplemented). It will be filled in alongside the rest of A1.2 by
// integrating `github.com/golang-jwt/jwt/v5` + `keyfunc/v3` once we've
// confirmed every upstream caller mints an audience-scoped token. Until
// then, setting `AUTHCTX_ENFORCE=1` is a deliberate fail-closed kill
// switch — the route returns 503 so a misconfigured rollout fails loudly
// instead of silently letting unauthenticated traffic through.
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
	UserID    string   `json:"user_id"`
	OrgID     string   `json:"org_id"`
	Email     string   `json:"email,omitempty"`
	Scopes    []string `json:"scopes,omitempty"`
	Issuer    string   `json:"iss"`
	Audience  string   `json:"aud"`
	Subject   string   `json:"sub"`
	IssuedAt  int64    `json:"iat"`
	NotBefore int64    `json:"nbf,omitempty"`
	ExpiresAt int64    `json:"exp"`
	// Verified is true when the signature + standard claims have been
	// checked against the JWKS. Observe-mode middleware reads + parses
	// JWTs without verifying — never trust an unverified Claims struct
	// for authorisation decisions.
	Verified bool `json:"-"`
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
		return false
	}
	return now.Unix() > c.ExpiresAt+30
}

type contextKey struct{}

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
	case "1", "true", "yes", "on":
		return true
	default:
		return false
	}
}

// ErrNotImplemented signals that enforce-mode verification is not wired
// up yet. The middleware returns 503 when it encounters this so a
// premature flip to enforce mode fails loudly.
var ErrNotImplemented = errors.New("authctx: signature verification not implemented yet")

// Middleware returns a chi-compatible middleware. In observe mode it
// decodes the JWT payload (no signature check) and stuffs Claims into
// the request context. In enforce mode it currently returns 503 with
// ErrNotImplemented — verification lands in the next commit.
func Middleware(cfg Config) func(http.Handler) http.Handler {
	enforce := cfg.resolveEnforce()
	audience := strings.TrimSpace(cfg.Audience)
	logger := log.With().Str("component", "authctx").Str("audience", audience).Logger()

	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			token := extractBearer(r)

			if enforce {
				// Fail closed. Once Verify() is implemented this branch
				// becomes the happy path; for now the kill switch must
				// never silently let unauthenticated traffic through.
				logger.Error().
					Bool("has_token", token != "").
					Msg("AUTHCTX_ENFORCE=1 but signature verification is not implemented; returning 503")
				http.Error(w, `{"error":"authctx enforce not yet implemented"}`, http.StatusServiceUnavailable)
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
