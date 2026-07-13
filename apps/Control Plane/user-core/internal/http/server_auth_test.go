package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

func newUserAuthProbeRouter(t *testing.T, authHandler http.HandlerFunc) *gin.Engine {
	return newUserAuthProbeRouterWithCredentials(t, authHandler, `[{"principal":"velion-gateway","audience":"user-core","token":"test-only-user-core-service-token","scopes":["users:read:self","users:write:self"]}]`)
}

func newUserAuthProbeRouterWithCredentials(t *testing.T, authHandler http.HandlerFunc, credentials string) *gin.Engine {
	t.Helper()
	authServer := httptest.NewServer(authHandler)
	t.Cleanup(authServer.Close)
	t.Setenv("AUTH_SERVICE_URL", authServer.URL)
	t.Setenv("INTERNAL_API_KEY", "shared-fleet-key")
	t.Setenv("INTERNAL_SERVICE_SECRET", "")
	t.Setenv("USER_CORE_SERVICE_CREDENTIALS", credentials)

	router := gin.New()
	router.Use(authContextMiddleware())
	router.GET("/admin", func(c *gin.Context) {
		if !isAdminRequest(c) {
			c.JSON(http.StatusForbidden, gin.H{"error": "admin role required"})
			return
		}
		userID, _ := c.Get("user_id")
		c.JSON(http.StatusOK, gin.H{"user_id": userID})
	})
	return router
}

func TestSharedFleetKeyCannotDelegateUserIdentity(t *testing.T) {
	router := newUserAuthProbeRouter(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called for a rejected shared key")
	})
	router.GET("/whoami", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{"user_id": c.GetString("user_id")})
	})

	response := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/whoami", nil)
	request.Header.Set("X-Internal-Api-Key", "shared-fleet-key")
	request.Header.Set("X-User-Id", "victim")
	router.ServeHTTP(response, request)

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("expected shared-key delegation to return 401, got %d", response.Code)
	}
}

func TestServiceCredentialCannotDelegateSelfFromHeaders(t *testing.T) {
	router := newUserAuthProbeRouter(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called for a service principal")
	})
	router.GET("/whoami", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{"user_id": c.GetString("user_id"), "service_id": c.GetString("service_id")})
	})
	router.POST("/whoami", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{"user_id": c.GetString("user_id")})
	})

	for _, test := range []struct {
		method     string
		wantStatus int
	}{
		{method: http.MethodGet, wantStatus: http.StatusForbidden},
		{method: http.MethodPost, wantStatus: http.StatusForbidden},
	} {
		response := httptest.NewRecorder()
		request := httptest.NewRequest(test.method, "/whoami", nil)
		request.Header.Set("X-Service-Token", "test-only-user-core-service-token")
		request.Header.Set("X-Service-Id", "velion-gateway")
		request.Header.Set("X-User-Id", "verified-at-gateway")
		router.ServeHTTP(response, request)
		if response.Code != test.wantStatus {
			t.Fatalf("%s status = %d; want %d", test.method, response.Code, test.wantStatus)
		}
	}
}

func TestServiceCredentialCannotSelectAuthzTenantSubjectOrGrantActor(t *testing.T) {
	router := newUserAuthProbeRouterWithCredentials(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called for a service principal")
	}, `[{"principal":"velion-gateway","audience":"user-core","token":"test-only-user-core-service-token","scopes":["authz:read","authz:write"]}]`)

	handlerCalled := false
	router.GET("/api/v1/internal/authz/visible", func(c *gin.Context) {
		handlerCalled = true
		c.Status(http.StatusOK)
	})
	router.POST("/api/v1/internal/authz/grant", func(c *gin.Context) {
		handlerCalled = true
		c.Status(http.StatusOK)
	})

	tests := []struct {
		name   string
		method string
		target string
		body   string
	}{
		{
			name:   "read cannot select arbitrary tenant and subject",
			method: http.MethodGet,
			target: "/api/v1/internal/authz/visible?org_id=victim-org&subject_id=victim-user&resource_type=document",
		},
		{
			name:   "write cannot select arbitrary tenant subject and grant actor",
			method: http.MethodPost,
			target: "/api/v1/internal/authz/grant",
			body:   `{"org_id":"victim-org","resource_type":"document","resource_id":"victim-document","subject_id":"attacker","granted_by":"victim-admin"}`,
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			handlerCalled = false
			response := httptest.NewRecorder()
			request := httptest.NewRequest(test.method, test.target, strings.NewReader(test.body))
			request.Header.Set("Content-Type", "application/json")
			request.Header.Set("X-Service-Token", "test-only-user-core-service-token")
			request.Header.Set("X-Service-Id", "velion-gateway")
			router.ServeHTTP(response, request)

			if response.Code != http.StatusForbidden {
				t.Fatalf("status = %d; want %d", response.Code, http.StatusForbidden)
			}
			if handlerCalled {
				t.Fatal("unbounded authz request reached the handler")
			}
		})
	}
}

func TestServiceCredentialRetainsExplicitNonDelegatedScope(t *testing.T) {
	router := newUserAuthProbeRouterWithCredentials(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called for a service principal")
	}, `[{"principal":"org-core","audience":"user-core","token":"test-only-user-core-service-token","scopes":["users:read:any"]}]`)
	router.GET("/service-read", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"service_id": c.GetString("service_id"),
			"user_id":    c.GetString("user_id"),
		})
	})

	response := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/service-read", nil)
	request.Header.Set("X-Service-Token", "test-only-user-core-service-token")
	request.Header.Set("X-Service-Id", "org-core")
	router.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d; want %d", response.Code, http.StatusOK)
	}
	var payload struct {
		ServiceID string `json:"service_id"`
		UserID    string `json:"user_id"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &payload); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if payload.ServiceID != "org-core" || payload.UserID != "" {
		t.Fatalf("unexpected non-delegated identity: %+v", payload)
	}
}

func TestSharedFleetKeyCannotSelfAssertAdminRole(t *testing.T) {
	router := newUserAuthProbeRouter(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Fatal("auth-core must not be called for the legacy internal key path")
	})

	response := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/admin", nil)
	request.Header.Set("X-Internal-Api-Key", "shared-fleet-key")
	request.Header.Set("X-User-Id", "attacker")
	request.Header.Set("X-User-Role", "admin")
	request.Header.Set("X-Auth-Role", "superadmin")
	router.ServeHTTP(response, request)

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("expected shared-key role assertion to be unauthorized, got %d", response.Code)
	}
}

func TestBearerRoleComesOnlyFromVerifiedAuthCoreIdentity(t *testing.T) {
	tests := []struct {
		name       string
		role       string
		wantStatus int
	}{
		{name: "verified member ignores forged admin header", role: "member", wantStatus: http.StatusForbidden},
		{name: "verified admin is authorized", role: "admin", wantStatus: http.StatusOK},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			router := newUserAuthProbeRouter(t, func(w http.ResponseWriter, request *http.Request) {
				if got := request.Header.Get("Authorization"); got != "Bearer verified-token" {
					t.Fatalf("expected verified bearer to be introspected, got %q", got)
				}
				w.Header().Set("Content-Type", "application/json")
				if err := json.NewEncoder(w).Encode(map[string]any{
					"user": map[string]any{"id": "verified-user", "role": test.role},
				}); err != nil {
					t.Fatalf("encode auth response: %v", err)
				}
			})

			response := httptest.NewRecorder()
			request := httptest.NewRequest(http.MethodGet, "/admin", nil)
			request.Header.Set("Authorization", "Bearer verified-token")
			request.Header.Set("X-User-Id", "attacker")
			request.Header.Set("X-User-Role", "admin")
			router.ServeHTTP(response, request)

			if response.Code != test.wantStatus {
				t.Fatalf("expected status %d, got %d", test.wantStatus, response.Code)
			}
		})
	}
}

func TestBearerProfileAttributesComeOnlyFromVerifiedAuthCoreIdentity(t *testing.T) {
	router := newUserAuthProbeRouter(t, func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		if err := json.NewEncoder(w).Encode(map[string]any{
			"user": map[string]any{
				"id":    "verified-user",
				"email": "verified@example.com",
				"name":  "Verified Name",
				"image": "https://images.example.com/verified.png",
			},
		}); err != nil {
			t.Fatalf("encode auth response: %v", err)
		}
	})
	router.GET("/identity", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"user_id":     c.GetString("user_id"),
			"user_email":  c.GetString("user_email"),
			"user_name":   c.GetString("user_name"),
			"user_avatar": c.GetString("user_avatar"),
		})
	})

	response := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/identity", nil)
	request.Header.Set("Authorization", "Bearer verified-token")
	request.Header.Set("X-User-Id", "victim-id")
	request.Header.Set("X-User-Email", "victim@example.com")
	request.Header.Set("X-User-Name", "Victim Name")
	request.Header.Set("X-User-Avatar", "https://images.example.com/victim.png")
	router.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d; want %d", response.Code, http.StatusOK)
	}
	var payload map[string]string
	if err := json.Unmarshal(response.Body.Bytes(), &payload); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if payload["user_id"] != "verified-user" ||
		payload["user_email"] != "verified@example.com" ||
		payload["user_name"] != "Verified Name" ||
		payload["user_avatar"] != "https://images.example.com/verified.png" {
		t.Fatalf("caller-supplied profile attributes escaped verification: %+v", payload)
	}
}

func TestHandlerProfileAttributesIgnoreCallerHeaders(t *testing.T) {
	context, _ := gin.CreateTestContext(httptest.NewRecorder())
	context.Request = httptest.NewRequest(http.MethodGet, "/api/v1/users/me", nil)
	context.Request.Header.Set("X-User-Email", "victim@example.com")
	context.Request.Header.Set("X-User-Name", "Victim Name")
	context.Request.Header.Set("X-User-Avatar", "https://images.example.com/victim.png")
	context.Set("user_email", "verified@example.com")
	context.Set("user_name", "Verified Name")
	context.Set("user_avatar", "https://images.example.com/verified.png")

	email, name, avatar := verifiedProfileFromContext(context)
	if email != "verified@example.com" ||
		name != "Verified Name" ||
		avatar != "https://images.example.com/verified.png" {
		t.Fatalf("handler accepted caller profile headers: email=%q name=%q avatar=%q", email, name, avatar)
	}
}
