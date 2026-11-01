// Package authctx verifies Auth Core JWTs and derives immutable tenant and
// actor identity for Model Plane HTTP and gRPC entry points.
package authctx

import (
	"context"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/big"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

const maxJWKSBytes = 1 << 20

type principalContextKey struct{}

// Principal is identity proven by an Auth Core signature. Request headers,
// query strings, and bodies are never used to construct it.
type Principal struct {
	OrganizationID string
	// ActorID is the user id of a user, the service id of a service, and for
	// an api_key principal the key's owning user (acting_user_id), used for
	// owner accounting only. An api_key principal is never a person: check
	// PrincipalType (or KeyID) before treating ActorID as one.
	ActorID                string
	PrincipalType          string
	Scopes                 []string
	ZeroDataRetention      bool
	RetentionPolicyPresent bool
	// KeyID is the API key id of an api_key principal (INNH-10), empty for
	// every other type. It names the key, it is never the key.
	KeyID string
	// KeyHostCeiling is an api_key token's key_hosts claim, as signed (not yet
	// normalized): the hosts a grant for this key may hold at most.
	KeyHostCeiling []string
	// KeyBudgetCeiling is an api_key token's key_budget claim: the most a
	// grant for this key may bound. Zero on a dimension means no ceiling was
	// signed for it, which a consumer must treat as refuse, not unbounded.
	KeyBudgetCeiling KeyBudget
}

// KeyBudget is the budget ceiling an api_key token carries (micro-USD, steps,
// wall-clock seconds).
type KeyBudget struct {
	MicroUSD    int64 `json:"micro_usd"`
	MaxSteps    int32 `json:"max_steps"`
	MaxRuntimeS int32 `json:"max_runtime_s"`
}

// PrincipalTypeAPIKey is the INNH-10 principal minted by Auth Core's API-key
// exchange. A Verifier accepts it only when its Config opts in (AcceptAPIKey).
const PrincipalTypeAPIKey = "api_key"

// APIKeySubjectPrefix prefixes an api_key token's sub: "apikey:<key_id>".
const APIKeySubjectPrefix = "apikey:"

const (
	maxKeyIDBytes      = 128
	maxKeyHostCeiling  = 32
	maxKeyHostNameSize = 253
)

// HasScope reports whether the immutable verified principal has scope.
func (p Principal) HasScope(scope string) bool {
	return slices.Contains(p.Scopes, scope)
}

// Config describes the Auth Core trust boundary. PublicKeyPEM is intended for
// tests and offline deployments; production normally uses JWKSURL.
type Config struct {
	Audiences    []string
	Issuer       string
	JWKSURL      string
	PublicKeyPEM []byte
	HTTPClient   *http.Client
	// AcceptAPIKey admits principal_type=api_key (INNH-10). Off by default, so
	// every service that does not opt in keeps refusing the type as
	// unsupported: none of them was written for a principal that is neither a
	// person nor a registered service.
	AcceptAPIKey bool
}

type claims struct {
	OrgID         string   `json:"org_id"`
	UserID        string   `json:"user_id"`
	ServiceID     string   `json:"service_id"`
	PrincipalType string   `json:"principal_type"`
	Scopes        []string `json:"scopes"`
	ZDR           *bool    `json:"zdr"`
	// INNH-10 api_key claims (Auth Core CP-1).
	KeyID        string     `json:"key_id"`
	ActingUserID string     `json:"acting_user_id"`
	KeyHosts     []string   `json:"key_hosts"`
	KeyBudget    *KeyBudget `json:"key_budget"`
	jwt.RegisteredClaims
}

// Verifier is immutable verification state loaded before a service starts.
type Verifier struct {
	audiences    []string
	issuer       string
	static       *rsa.PublicKey
	keys         map[string]*rsa.PublicKey
	acceptAPIKey bool
}

// HTTPAuthorizer performs service-specific authorization after authentication.
// Returning an error denies the request without exposing the error text.
type HTTPAuthorizer func(Principal, *http.Request) error

// GRPCAuthorizer performs service-specific authorization after authentication.
type GRPCAuthorizer func(Principal, string, any) error

// NewVerifier validates trust material eagerly so a sensitive service cannot
// start in an unauthenticated fallback mode.
func NewVerifier(config Config) (*Verifier, error) {
	audiences := normalizedUnique(config.Audiences)
	issuer := strings.TrimSpace(config.Issuer)
	if len(audiences) == 0 || issuer == "" {
		return nil, errors.New("authctx: at least one audience and an issuer are required")
	}
	verifier := &Verifier{
		audiences:    audiences,
		issuer:       issuer,
		keys:         map[string]*rsa.PublicKey{},
		acceptAPIKey: config.AcceptAPIKey,
	}
	if len(config.PublicKeyPEM) > 0 {
		key, err := jwt.ParseRSAPublicKeyFromPEM(config.PublicKeyPEM)
		if err != nil {
			return nil, fmt.Errorf("authctx: parse public key: %w", err)
		}
		verifier.static = key
	}
	if strings.TrimSpace(config.JWKSURL) != "" {
		keys, err := loadJWKS(config)
		if err != nil {
			return nil, err
		}
		verifier.keys = keys
	}
	if verifier.static == nil && len(verifier.keys) == 0 {
		return nil, errors.New("authctx: public key or JWKS URL is required")
	}
	return verifier, nil
}

// Verify validates signature, algorithm, issuer, lifetime, audience, and an
// unambiguous user or service identity (or, when the Config opted in, an
// api_key identity of the exact INNH-10 shape).
func (v *Verifier) Verify(raw string) (Principal, error) {
	parsedClaims := &claims{}
	parsed, err := jwt.ParseWithClaims(
		raw,
		parsedClaims,
		func(token *jwt.Token) (any, error) {
			if token.Method.Alg() != jwt.SigningMethodRS256.Alg() {
				return nil, fmt.Errorf("unexpected signing algorithm %q", token.Method.Alg())
			}
			if keyID, _ := token.Header["kid"].(string); keyID != "" {
				if key := v.keys[keyID]; key != nil {
					return key, nil
				}
			}
			if v.static != nil {
				return v.static, nil
			}
			return nil, errors.New("signing key not found")
		},
		jwt.WithValidMethods([]string{jwt.SigningMethodRS256.Alg()}),
		jwt.WithIssuer(v.issuer),
		jwt.WithExpirationRequired(),
		jwt.WithIssuedAt(),
		jwt.WithLeeway(30*time.Second),
	)
	if err != nil || !parsed.Valid {
		return Principal{}, errors.New("authentication token is invalid")
	}
	if parsedClaims.IssuedAt == nil || parsedClaims.NotBefore == nil ||
		strings.TrimSpace(parsedClaims.OrgID) == "" ||
		!hasAcceptedAudience(parsedClaims.Audience, v.audiences) {
		return Principal{}, errors.New("authentication token is missing required claims")
	}

	principal := Principal{
		OrganizationID:         strings.TrimSpace(parsedClaims.OrgID),
		PrincipalType:          strings.TrimSpace(parsedClaims.PrincipalType),
		Scopes:                 append([]string(nil), parsedClaims.Scopes...),
		RetentionPolicyPresent: parsedClaims.ZDR != nil,
	}
	if parsedClaims.ZDR != nil {
		principal.ZeroDataRetention = *parsedClaims.ZDR
	}
	subject := strings.TrimSpace(parsedClaims.Subject)
	// key_id names an API key; on any other principal type it would make the
	// identity ambiguous, so it is refused rather than ignored.
	if principal.PrincipalType != PrincipalTypeAPIKey && parsedClaims.KeyID != "" {
		return Principal{}, errors.New("authentication token has ambiguous identity")
	}
	switch principal.PrincipalType {
	case PrincipalTypeAPIKey:
		if !v.acceptAPIKey {
			return Principal{}, errors.New("authentication token has unsupported principal type")
		}
		keyPrincipal, err := apiKeyPrincipal(parsedClaims)
		if err != nil {
			return Principal{}, err
		}
		principal.ActorID = keyPrincipal.ActorID
		principal.KeyID = keyPrincipal.KeyID
		principal.KeyHostCeiling = keyPrincipal.KeyHostCeiling
		principal.KeyBudgetCeiling = keyPrincipal.KeyBudgetCeiling
	case "user":
		principal.ActorID = strings.TrimSpace(parsedClaims.UserID)
		if principal.ActorID == "" || strings.TrimSpace(parsedClaims.ServiceID) != "" || subject != principal.ActorID {
			return Principal{}, errors.New("authentication token has ambiguous user identity")
		}
	case "service":
		principal.ActorID = strings.TrimSpace(parsedClaims.ServiceID)
		if principal.ActorID == "" || strings.TrimSpace(parsedClaims.UserID) != "" ||
			subject != principal.ActorID || len(principal.Scopes) == 0 {
			return Principal{}, errors.New("authentication token has ambiguous service identity")
		}
	default:
		return Principal{}, errors.New("authentication token has unsupported principal type")
	}
	return principal, nil
}

// apiKeyPrincipal checks the INNH-10 api_key token shape exactly: sub is
// "apikey:<key_id>", key_id is 1 to 128 printable non-space ASCII bytes,
// acting_user_id (the key's owner) is present, and user_id and service_id are
// absent. The key's host ceiling is bounded (at most 32 names, each at most 253
// bytes) and a budget ceiling may not be negative. Anything else is refused.
func apiKeyPrincipal(c *claims) (Principal, error) {
	ambiguous := errors.New("authentication token has ambiguous api key identity")
	if !validKeyID(c.KeyID) || c.Subject != APIKeySubjectPrefix+c.KeyID {
		return Principal{}, ambiguous
	}
	owner := strings.TrimSpace(c.ActingUserID)
	if owner == "" || owner != c.ActingUserID || strings.HasPrefix(owner, APIKeySubjectPrefix) ||
		strings.TrimSpace(c.UserID) != "" || strings.TrimSpace(c.ServiceID) != "" {
		return Principal{}, ambiguous
	}
	if len(c.KeyHosts) > maxKeyHostCeiling {
		return Principal{}, errors.New("authentication token has an unbounded api key host ceiling")
	}
	for _, host := range c.KeyHosts {
		if host == "" || len(host) > maxKeyHostNameSize {
			return Principal{}, errors.New("authentication token has an unbounded api key host ceiling")
		}
	}
	principal := Principal{
		ActorID:        owner,
		KeyID:          c.KeyID,
		KeyHostCeiling: append([]string(nil), c.KeyHosts...),
	}
	if c.KeyBudget != nil {
		if c.KeyBudget.MicroUSD < 0 || c.KeyBudget.MaxSteps < 0 || c.KeyBudget.MaxRuntimeS < 0 {
			return Principal{}, errors.New("authentication token has a negative api key budget ceiling")
		}
		principal.KeyBudgetCeiling = *c.KeyBudget
	}
	return principal, nil
}

// validKeyID reports a bounded, printable key id (0x21-0x7e, no space).
func validKeyID(id string) bool {
	if id == "" || len(id) > maxKeyIDBytes {
		return false
	}
	for i := 0; i < len(id); i++ {
		if id[i] < 0x21 || id[i] > 0x7e {
			return false
		}
	}
	return true
}

// HTTPMiddleware authenticates a bearer token, rejects forged identity
// headers, canonicalizes downstream identity headers, and runs authorizer.
func (v *Verifier) HTTPMiddleware(authorizer HTTPAuthorizer) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			raw := bearerToken(r.Header.Get("Authorization"))
			if raw == "" {
				writeHTTPAuthError(w, http.StatusUnauthorized, "authentication required")
				return
			}
			principal, err := v.Verify(raw)
			if err != nil {
				writeHTTPAuthError(w, http.StatusUnauthorized, "invalid authentication token")
				return
			}
			// The HTTP path forwards X-User-ID downstream, which would name the
			// key's owner as if the person had called. No HTTP entry point is
			// built for an api_key principal (INNH-10 is gRPC to the broker), so
			// it is refused here even when the verifier opted in.
			if principal.PrincipalType == PrincipalTypeAPIKey {
				writeHTTPAuthError(w, http.StatusForbidden, "request is not authorized")
				return
			}
			if conflicts(r.Header.Get("X-Org-ID"), principal.OrganizationID) ||
				conflicts(r.Header.Get("X-User-ID"), principal.ActorID) {
				writeHTTPAuthError(w, http.StatusForbidden, "identity context does not match verified token")
				return
			}
			if authorizer != nil {
				if err := authorizer(principal, r); err != nil {
					writeHTTPAuthError(w, http.StatusForbidden, "request is not authorized")
					return
				}
			}

			request := r.Clone(withPrincipal(r.Context(), principal))
			request.Header = r.Header.Clone()
			request.Header.Set("X-Org-ID", principal.OrganizationID)
			request.Header.Set("X-User-ID", principal.ActorID)
			next.ServeHTTP(w, request)
		})
	}
}

// UnaryServerInterceptor authenticates gRPC metadata and adds the verified
// principal to the handler context. Services must pin request tenant fields to
// PrincipalFromContext, normally through authorizer.
func (v *Verifier) UnaryServerInterceptor(authorizer GRPCAuthorizer) grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
		values, _ := metadata.FromIncomingContext(ctx)
		raw := bearerToken(firstMetadataValue(values, "authorization"))
		if raw == "" {
			return nil, status.Error(codes.Unauthenticated, "authentication required")
		}
		principal, err := v.Verify(raw)
		if err != nil {
			return nil, status.Error(codes.Unauthenticated, "invalid authentication token")
		}
		if conflicts(firstMetadataValue(values, "x-org-id"), principal.OrganizationID) ||
			conflicts(firstMetadataValue(values, "x-user-id"), principal.ActorID) {
			return nil, status.Error(codes.PermissionDenied, "identity context does not match verified token")
		}
		if authorizer != nil {
			if err := authorizer(principal, info.FullMethod, req); err != nil {
				return nil, status.Error(codes.PermissionDenied, "request is not authorized")
			}
		}
		return handler(withPrincipal(ctx, principal), req)
	}
}

// PrincipalFromContext returns only cryptographically verified identity.
func PrincipalFromContext(ctx context.Context) (Principal, bool) {
	principal, ok := ctx.Value(principalContextKey{}).(Principal)
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		return Principal{}, false
	}
	principal.Scopes = append([]string(nil), principal.Scopes...)
	principal.KeyHostCeiling = append([]string(nil), principal.KeyHostCeiling...)
	return principal, true
}

// ContextWithPrincipal places a verified identity on a context.
//
// The exported counterpart to [PrincipalFromContext], which existed alone: this
// package's whole job is carrying a principal through a context, and being able
// to read one but not write one made every downstream handler untestable
// without standing up the full verifier and a signed token.
//
// It performs NO verification, which is exactly why production code must not
// call it — the middleware and interceptor above are the only things that
// should, and they do so through the unexported form. Its use is test setup:
// an in-package handler test that needs "a request from this member" rather
// than a round trip through Auth Core.
func ContextWithPrincipal(ctx context.Context, principal Principal) context.Context {
	return withPrincipal(ctx, principal)
}

func withPrincipal(ctx context.Context, principal Principal) context.Context {
	copy := principal
	copy.Scopes = append([]string(nil), principal.Scopes...)
	copy.KeyHostCeiling = append([]string(nil), principal.KeyHostCeiling...)
	return context.WithValue(ctx, principalContextKey{}, copy)
}

func normalizedUnique(values []string) []string {
	result := make([]string, 0, len(values))
	for _, value := range values {
		value = strings.TrimSpace(value)
		if value != "" && !slices.Contains(result, value) {
			result = append(result, value)
		}
	}
	return result
}

func hasAcceptedAudience(tokenAudiences, accepted []string) bool {
	for _, candidate := range tokenAudiences {
		if slices.Contains(accepted, candidate) {
			return true
		}
	}
	return false
}

func bearerToken(header string) string {
	parts := strings.Fields(header)
	if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") {
		return ""
	}
	return strings.TrimSpace(parts[1])
}

func conflicts(unverified, verified string) bool {
	unverified = strings.TrimSpace(unverified)
	return unverified != "" && unverified != verified
}

func firstMetadataValue(values metadata.MD, key string) string {
	items := values.Get(key)
	if len(items) == 0 {
		return ""
	}
	return items[0]
}

func writeHTTPAuthError(w http.ResponseWriter, code int, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(map[string]any{
		"error": map[string]any{
			"code":    "AUTHORIZATION_FAILED",
			"message": message,
		},
	})
}

type jwksDocument struct {
	Keys []struct {
		KeyID string `json:"kid"`
		Type  string `json:"kty"`
		Use   string `json:"use"`
		Alg   string `json:"alg"`
		N     string `json:"n"`
		E     string `json:"e"`
	} `json:"keys"`
}

func loadJWKS(config Config) (map[string]*rsa.PublicKey, error) {
	parsed, err := url.ParseRequestURI(strings.TrimSpace(config.JWKSURL))
	if err != nil || parsed.Host == "" || parsed.User != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") {
		return nil, errors.New("authctx: JWKS URL must be an http(s) URL without user info")
	}
	client := config.HTTPClient
	if client == nil {
		client = &http.Client{Timeout: 5 * time.Second}
	}
	copy := *client
	if copy.Timeout <= 0 || copy.Timeout > 10*time.Second {
		copy.Timeout = 5 * time.Second
	}
	copy.CheckRedirect = func(_ *http.Request, _ []*http.Request) error {
		return http.ErrUseLastResponse
	}
	request, err := http.NewRequestWithContext(context.Background(), http.MethodGet, parsed.String(), nil)
	if err != nil {
		return nil, fmt.Errorf("authctx: build JWKS request: %w", err)
	}
	response, err := copy.Do(request)
	if err != nil {
		return nil, fmt.Errorf("authctx: fetch JWKS: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("authctx: JWKS returned HTTP %d", response.StatusCode)
	}
	var document jwksDocument
	decoder := json.NewDecoder(io.LimitReader(response.Body, maxJWKSBytes))
	if err := decoder.Decode(&document); err != nil {
		return nil, fmt.Errorf("authctx: decode JWKS: %w", err)
	}
	keys := make(map[string]*rsa.PublicKey, len(document.Keys))
	for _, item := range document.Keys {
		if item.KeyID == "" || item.Type != "RSA" || item.Alg != "RS256" || (item.Use != "" && item.Use != "sig") {
			continue
		}
		n, nErr := base64.RawURLEncoding.DecodeString(item.N)
		e, eErr := base64.RawURLEncoding.DecodeString(item.E)
		if nErr != nil || eErr != nil || len(n) == 0 || len(e) == 0 {
			continue
		}
		exponent := int(new(big.Int).SetBytes(e).Int64())
		if exponent < 3 {
			continue
		}
		keys[item.KeyID] = &rsa.PublicKey{N: new(big.Int).SetBytes(n), E: exponent}
	}
	if len(keys) == 0 {
		return nil, errors.New("authctx: JWKS contains no usable RS256 keys")
	}
	return keys, nil
}
