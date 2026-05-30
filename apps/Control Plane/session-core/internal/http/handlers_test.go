package http

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestMain(m *testing.M) {
	gin.SetMode(gin.TestMode)
	os.Exit(m.Run())
}

// newTestServer creates a Server with all external deps nil, suitable for
// tests that do not reach service/NATS/Redis calls.
func newTestServer() *Server {
	return NewServer(nil, nil, nil, nil, "")
}

// addBearerAuth sets the headers required for Branch 2 of authContextMiddleware
// to fire, ensuring getUserID(c) returns a non-empty string.
func addBearerAuth(req *http.Request, userID string) {
	req.Header.Set("Authorization", "Bearer testtoken")
	req.Header.Set("X-User-Id", userID)
}

// --- Health Check ---

func TestHealthCheck_Returns200(t *testing.T) {
	srv := newTestServer()
	w := httptest.NewRecorder()
	req := httptest.NewRequest(http.MethodGet, "/health", nil)

	srv.router.ServeHTTP(w, req)

	assert.Equal(t, http.StatusOK, w.Code)

	var body map[string]interface{}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &body))
	assert.Equal(t, "healthy", body["status"])
	assert.Equal(t, "session-core", body["service"])
}

func TestHealthCheck_BodyContainsVersion(t *testing.T) {
	srv := newTestServer()
	w := httptest.NewRecorder()
	req := httptest.NewRequest(http.MethodGet, "/health", nil)

	srv.router.ServeHTTP(w, req)

	var body map[string]interface{}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &body))
	assert.NotEmpty(t, body["version"], "version field should be present")
}

// --- createSession: auth guard ---

func TestCreateSession_NoAuthHeaders_Returns401(t *testing.T) {
	srv := newTestServer()
	w := httptest.NewRecorder()
	req := httptest.NewRequest(http.MethodPost, "/v1/sessions", bytes.NewBufferString(`{}`))
	req.Header.Set("Content-Type", "application/json")

	srv.router.ServeHTTP(w, req)

	assert.Equal(t, http.StatusUnauthorized, w.Code)

	var body map[string]interface{}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &body))
	assert.Equal(t, "authentication required", body["error"])
}

func TestCreateSession_BearerAuthMissingUserID_Returns401(t *testing.T) {
	// Authorization header present but X-User-Id missing → middleware Branch 2
	// condition fails → user_id never set → handler returns 401.
	srv := newTestServer()
	w := httptest.NewRecorder()
	req := httptest.NewRequest(http.MethodPost, "/v1/sessions", bytes.NewBufferString(`{}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer testtoken")
	// X-User-Id intentionally omitted

	srv.router.ServeHTTP(w, req)

	assert.Equal(t, http.StatusUnauthorized, w.Code)
}

// --- createSession: validation ---

func TestCreateSession_AuthWithMalformedJSON_Returns400(t *testing.T) {
	srv := newTestServer()
	w := httptest.NewRecorder()
	req := httptest.NewRequest(http.MethodPost, "/v1/sessions", bytes.NewBufferString(`not-json`))
	req.Header.Set("Content-Type", "application/json")
	addBearerAuth(req, "user-abc")

	srv.router.ServeHTTP(w, req)

	assert.Equal(t, http.StatusBadRequest, w.Code)

	var body map[string]interface{}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &body))
	assert.Equal(t, "invalid request body", body["error"])
	assert.NotEmpty(t, body["details"])
}

func TestCreateSession_AuthWithEmptyBody_Returns400(t *testing.T) {
	srv := newTestServer()
	w := httptest.NewRecorder()
	// Empty body with content-type JSON is valid JSON null/empty → ShouldBindJSON
	// fails because the body is truly empty (not even "{}").
	req := httptest.NewRequest(http.MethodPost, "/v1/sessions", http.NoBody)
	req.Header.Set("Content-Type", "application/json")
	addBearerAuth(req, "user-abc")

	srv.router.ServeHTTP(w, req)

	// Empty body causes EOF from ShouldBindJSON
	assert.Equal(t, http.StatusBadRequest, w.Code)
}

// --- getUserID helper ---

func TestGetUserID_SetString_ReturnsValue(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Set("user_id", "test-user-999")

	result := getUserID(c)

	assert.Equal(t, "test-user-999", result)
}

func TestGetUserID_NotSet_ReturnsEmpty(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)

	result := getUserID(c)

	assert.Equal(t, "", result)
}

func TestGetUserID_SetNonString_ReturnsEmpty(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Set("user_id", 12345) // wrong type

	result := getUserID(c)

	assert.Equal(t, "", result)
}

func TestGetUserID_SetEmptyString_ReturnsEmpty(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Set("user_id", "") // empty string

	result := getUserID(c)

	assert.Equal(t, "", result)
}
