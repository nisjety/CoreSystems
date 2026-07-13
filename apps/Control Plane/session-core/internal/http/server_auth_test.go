package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func newAuthProbeRouter(t *testing.T, authHandler http.HandlerFunc) *gin.Engine {
	t.Helper()

	authServer := httptest.NewServer(authHandler)
	t.Cleanup(authServer.Close)
	t.Setenv("AUTH_SERVICE_URL", authServer.URL)
	t.Setenv("INTERNAL_API_KEY", "shared-fleet-key")
	t.Setenv("INTERNAL_SERVICE_SECRET", "")

	router := gin.New()
	router.Use(authContextMiddleware())
	router.GET("/protected", func(c *gin.Context) {
		writeAuthProbe(c)
	})
	router.GET("/api/v1/sessions/current", func(c *gin.Context) {
		writeAuthProbe(c)
	})
	router.POST("/api/v1/sessions/refresh", func(c *gin.Context) {
		writeAuthProbe(c)
	})
	return router
}

func writeAuthProbe(c *gin.Context) {
	userID, _ := c.Get("user_id")
	authMethod, _ := c.Get("auth_method")
	servicePrincipal, _ := c.Get("service_principal")
	c.JSON(http.StatusOK, gin.H{
		"user_id":           userID,
		"auth_method":       authMethod,
		"service_principal": servicePrincipal,
	})
}

func requestProtected(t *testing.T, router http.Handler, headers map[string]string) *httptest.ResponseRecorder {
	t.Helper()

	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/protected", nil)
	for name, value := range headers {
		request.Header.Set(name, value)
	}
	router.ServeHTTP(recorder, request)
	return recorder
}

func signSessionRequest(request *http.Request, token, principal, userID string, timestamp time.Time) {
	timestampValue := timestamp.UTC().Format(time.RFC3339)
	bodyDigest := sessionDelegationBodyDigest(nil)
	claims := sessionDelegationClaims{
		Principal: principal, Audience: serviceCredentialAudience,
		Timestamp: timestampValue, Method: request.Method, URI: request.URL.RequestURI(),
		UserID: userID, BodySHA256: bodyDigest,
	}
	request.Header.Set("X-Service-Token", token)
	request.Header.Set("X-User-Id", userID)
	request.Header.Set("X-Delegation-Timestamp", timestampValue)
	request.Header.Set("X-Delegation-Body-SHA256", bodyDigest)
	request.Header.Set("X-Delegation-Signature", sessionDelegationSignature(token, claims))
}

func TestAuthContextRejectsUnverifiedBearerIdentities(t *testing.T) {
	rejectedTokens := map[string]string{
		"garbage":         "malformed token",
		"expired":         "expired token",
		"wrong-issuer":    "wrong issuer",
		"wrong-audience":  "wrong audience",
		"wrong-signature": "wrong signature",
	}

	router := newAuthProbeRouter(t, func(w http.ResponseWriter, request *http.Request) {
		assert.Contains(t, rejectedTokens, request.Header.Get("Authorization")[len("Bearer "):])
		http.Error(w, "invalid session", http.StatusUnauthorized)
	})

	for token, name := range rejectedTokens {
		t.Run(name, func(t *testing.T) {
			response := requestProtected(t, router, map[string]string{
				"Authorization": "Bearer " + token,
				"X-User-Id":     "victim-user",
				"X-User-Role":   "admin",
			})
			assert.Equal(t, http.StatusUnauthorized, response.Code)
		})
	}
}

func TestAuthContextRejectsMissingOrMalformedBearerWithIdentityHeaders(t *testing.T) {
	router := newAuthProbeRouter(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called without a syntactically present bearer token")
	})

	for name, authorization := range map[string]string{
		"missing":    "",
		"empty":      "Bearer   ",
		"wrong type": "Basic Zm9yZ2Vk",
	} {
		t.Run(name, func(t *testing.T) {
			response := requestProtected(t, router, map[string]string{
				"Authorization": authorization,
				"X-User-Id":     "victim-user",
				"X-User-Role":   "admin",
			})
			assert.Equal(t, http.StatusUnauthorized, response.Code)
		})
	}
}

func TestAuthContextPinsIdentityToAuthCoreResponse(t *testing.T) {
	router := newAuthProbeRouter(t, func(w http.ResponseWriter, request *http.Request) {
		require.Equal(t, "Bearer valid-token", request.Header.Get("Authorization"))
		w.Header().Set("Content-Type", "application/json")
		require.NoError(t, json.NewEncoder(w).Encode(map[string]any{
			"user": map[string]any{"id": "verified-user", "role": "member"},
		}))
	})

	response := requestProtected(t, router, map[string]string{
		"Authorization": "Bearer valid-token",
		"X-User-Id":     "victim-user",
		"X-User-Role":   "admin",
	})

	require.Equal(t, http.StatusOK, response.Code)
	var body map[string]any
	require.NoError(t, json.Unmarshal(response.Body.Bytes(), &body))
	assert.Equal(t, "verified-user", body["user_id"])
	assert.Equal(t, "bearer", body["auth_method"])
}

func TestAuthContextRejectsFleetSharedKeyAsUserDelegation(t *testing.T) {
	router := newAuthProbeRouter(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called for an internal credential")
	})

	response := requestProtected(t, router, map[string]string{
		"X-Internal-Api-Key": "shared-fleet-key",
		"X-User-Id":          "victim-user",
		"X-User-Role":        "admin",
	})

	assert.Equal(t, http.StatusUnauthorized, response.Code)
}

func TestAuthContextAcceptsAudienceAndScopeBoundServiceCredential(t *testing.T) {
	t.Setenv("SESSION_CORE_SERVICE_CREDENTIALS", `[{"principal":"velion-gateway","audience":"session-core","token":"0123456789abcdef0123456789abcdef","scopes":["sessions:read"]}]`)
	router := newAuthProbeRouter(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called for an internal service credential")
	})

	response := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/api/v1/sessions/current", nil)
	signSessionRequest(request, "0123456789abcdef0123456789abcdef", "velion-gateway", "verified-at-gateway", time.Now())
	router.ServeHTTP(response, request)

	require.Equal(t, http.StatusOK, response.Code)
	var body map[string]any
	require.NoError(t, json.Unmarshal(response.Body.Bytes(), &body))
	assert.Equal(t, "verified-at-gateway", body["user_id"])
	assert.Equal(t, "service_principal", body["auth_method"])
	assert.Equal(t, "velion-gateway", body["service_principal"])
}

func TestAuthContextRejectsUnsignedServiceDelegation(t *testing.T) {
	t.Setenv("SESSION_CORE_SERVICE_CREDENTIALS", `[{"principal":"velion-gateway","audience":"session-core","token":"0123456789abcdef0123456789abcdef","scopes":["sessions:read"]}]`)
	router := newAuthProbeRouter(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called for an internal service credential")
	})

	response := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/api/v1/sessions/current", nil)
	request.Header.Set("X-Service-Token", "0123456789abcdef0123456789abcdef")
	request.Header.Set("X-User-Id", "victim-user")
	router.ServeHTTP(response, request)

	assert.Equal(t, http.StatusForbidden, response.Code)
}

func TestSessionDelegationMatchesGatewayFixedVector(t *testing.T) {
	credential := serviceCredential{
		Principal: "velion-gateway",
		Audience:  "session-core",
		Token:     "0123456789abcdef0123456789abcdef",
	}
	request := httptest.NewRequest(http.MethodGet, "/api/v1/sessions/current", nil)
	request.Header.Set("X-User-Id", "verified-user")
	request.Header.Set("X-User-Email", "verified@example.com")
	request.Header.Set("X-User-Name", "Verified User")
	request.Header.Set("X-Delegation-Timestamp", "2026-07-11T02:00:00+00:00")
	request.Header.Set("X-Delegation-Body-SHA256", "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU")
	request.Header.Set("X-Delegation-Signature", "4dgUVZ5Z-eyZrNkVl-GrS5j7MfOHTDna2V8_vSRBbcA")
	now := time.Date(2026, 7, 11, 2, 0, 1, 0, time.UTC)

	claims, ok := verifySessionServiceDelegation(request, credential, now)
	require.True(t, ok)
	assert.Equal(t, "verified-user", claims.UserID)
	assert.Equal(t, "verified@example.com", claims.Email)
}

func TestAuthContextRejectsServiceCredentialWithoutRouteScope(t *testing.T) {
	t.Setenv("SESSION_CORE_SERVICE_CREDENTIALS", `[{"principal":"read-only-worker","audience":"session-core","token":"0123456789abcdef0123456789abcdef","scopes":["sessions:read"]}]`)
	router := newAuthProbeRouter(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called for an internal service credential")
	})

	response := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodPost, "/api/v1/sessions/refresh", nil)
	request.Header.Set("X-Service-Token", "0123456789abcdef0123456789abcdef")
	request.Header.Set("X-User-Id", "verified-at-gateway")
	router.ServeHTTP(response, request)

	assert.Equal(t, http.StatusForbidden, response.Code)
}
