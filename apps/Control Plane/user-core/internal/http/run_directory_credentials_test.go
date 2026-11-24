package http

import (
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

// A-REC-03: the run directory read through the real credential store. The
// route tests above inject service_scopes directly; these parse the committed
// USER_CORE_SERVICE_CREDENTIALS registry from the Control compose file, run
// authenticateServicePrincipal on the request and only then the route, so a
// registry that forgets the execution-core run-directory row fails here.

const runDirectoryTokenEnv = "EXECUTION_CORE_CONTROL_RUN_DIRECTORY_TOKEN"

var composeTokenPlaceholder = regexp.MustCompile(`\$\{([A-Z0-9_]+):\?[^}]*\}`)

// composeServiceCredentials returns the committed registry with every
// `${NAME:?...}` placeholder replaced by a distinct test token, and the token
// chosen for each name.
func composeServiceCredentials(t *testing.T) ([]serviceCredential, map[string]string) {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("..", "..", "..", "docker-compose.yml"))
	if err != nil {
		t.Fatalf("read the Control compose file: %v", err)
	}
	lines := strings.Split(strings.ReplaceAll(string(raw), "\r\n", "\n"), "\n")
	registry := ""
	for i, line := range lines {
		if strings.TrimSpace(line) == "USER_CORE_SERVICE_CREDENTIALS: >-" && i+1 < len(lines) {
			registry = strings.TrimSpace(lines[i+1])
			break
		}
	}
	if registry == "" {
		t.Fatal("USER_CORE_SERVICE_CREDENTIALS is missing from the Control compose file")
	}
	tokens := map[string]string{}
	registry = composeTokenPlaceholder.ReplaceAllStringFunc(registry, func(match string) string {
		name := composeTokenPlaceholder.FindStringSubmatch(match)[1]
		token := "test-token-" + strings.Repeat(strings.ToLower(name[:1]), 8) + "-" + strings.ToLower(name)
		for len(token) < 40 {
			token += "x"
		}
		tokens[name] = token
		return token
	})
	credentials, err := parseServiceCredentials(registry)
	if err != nil {
		t.Fatalf("parse the committed registry: %v", err)
	}
	return credentials, tokens
}

func runDirectoryCallThroughTheStore(s *Server, credentials []serviceCredential, principal, token string) *httptest.ResponseRecorder {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	engine.Use(func(c *gin.Context) {
		if _, authorized := authenticateServicePrincipal(c, credentials); !authorized {
			c.AbortWithStatus(http.StatusUnauthorized)
		}
	})
	group := engine.Group("/api/v1/internal/spaces")
	for _, register := range spaceFeatureRouteRegistrars {
		register(s, group)
	}
	request := httptest.NewRequest(http.MethodPost, runDirectoryPath, strings.NewReader(runDirectoryValidBody))
	request.Header.Set("X-Service-Id", principal)
	request.Header.Set("X-Service-Token", token)
	w := httptest.NewRecorder()
	engine.ServeHTTP(w, request)
	return w
}

func TestTheCommittedRegistryGrantsExecutionCoreTheRunDirectoryReadAlone(t *testing.T) {
	credentials, tokens := composeServiceCredentials(t)
	token, ok := tokens[runDirectoryTokenEnv]
	if !ok {
		t.Fatalf("the registry carries no %s row", runDirectoryTokenEnv)
	}
	holders := 0
	for _, credential := range credentials {
		if !credentialHasScope(credential, runDirectoryReadScope) {
			continue
		}
		holders++
		if credential.Principal != "execution-core" || credential.Token != token || len(credential.Scopes) != 1 {
			t.Fatalf("%s is held by %+v; want execution-core with its own token and no other scope", runDirectoryReadScope, credential)
		}
	}
	if holders != 1 {
		t.Fatalf("%d registry rows hold %s, want 1", holders, runDirectoryReadScope)
	}
	if serviceScopeRequiresVerifiedDelegation(runDirectoryReadScope) {
		t.Fatal("the run-bound read must not need a user delegation: the run names the subject")
	}
}

func TestTheRunDirectoryTokenReachesTheReadThroughTheStore(t *testing.T) {
	credentials, tokens := composeServiceCredentials(t)
	fx := newRunDirectoryFixture(t)
	w := runDirectoryCallThroughTheStore(fx.server, credentials, "execution-core", tokens[runDirectoryTokenEnv])
	if w.Code != http.StatusOK {
		t.Fatalf("want 200 through the credential store, got %d (%s)", w.Code, w.Body.String())
	}
	if fx.authority.calls != 1 {
		t.Fatalf("Session Core calls %d", fx.authority.calls)
	}
}

func TestExecutionCoresOtherControlTokensDoNotOpenTheRunDirectory(t *testing.T) {
	credentials, tokens := composeServiceCredentials(t)
	for _, name := range []string{"EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN", "EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN"} {
		token, ok := tokens[name]
		if !ok {
			t.Fatalf("the registry carries no %s row", name)
		}
		fx := newRunDirectoryFixture(t)
		w := runDirectoryCallThroughTheStore(fx.server, credentials, "execution-core", token)
		if w.Code != http.StatusUnauthorized || fx.authority.calls != 0 {
			t.Errorf("%s: %d, Session Core calls %d", name, w.Code, fx.authority.calls)
		}
	}
	// The right token under another principal name is refused too.
	fx := newRunDirectoryFixture(t)
	w := runDirectoryCallThroughTheStore(fx.server, credentials, "verevon-gateway", tokens[runDirectoryTokenEnv])
	if w.Code != http.StatusUnauthorized || fx.authority.calls != 0 {
		t.Errorf("gateway with the directory token: %d", w.Code)
	}
}

func TestTheRunControlPlaneScriptMintsTheRunDirectoryToken(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("..", "..", "..", "scripts", "run-control-plane.sh"))
	if err != nil {
		t.Fatalf("read run-control-plane.sh: %v", err)
	}
	for _, line := range strings.Split(strings.ReplaceAll(string(raw), "\r\n", "\n"), "\n") {
		if strings.TrimSpace(line) == runDirectoryTokenEnv {
			return
		}
	}
	t.Fatalf("run-control-plane.sh does not mint %s, so the compose registry cannot interpolate", runDirectoryTokenEnv)
}
