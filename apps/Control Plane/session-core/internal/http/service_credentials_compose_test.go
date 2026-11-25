package http

import (
	"bytes"
	"encoding/json"
	"os"
	"regexp"
	"strings"
	"testing"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
)

// serviceCredentialsComposePath is apps/Control Plane/docker-compose.yml, seen from this package.
const serviceCredentialsComposePath = "../../../docker-compose.yml"

// servicePrincipalsPath is apps/Control Plane/config/plane-service-principals.json, seen from this package.
const servicePrincipalsPath = "../../../config/plane-service-principals.json"

// The fixture is a low-entropy fake. It is at least 32 characters, session-core's minimum token length.
const composeFixtureGatewayToken = "fixture-verevon-gateway-session-token-0000000000"

// composeVariable matches ${NAME}, ${NAME:-default} and ${NAME:?message}.
var composeVariable = regexp.MustCompile(`\$\{([A-Za-z0-9_]+)(:-[^}]*|:\?[^}]*)?\}`)

// serviceCredentialsComposeLine returns the value line of SESSION_CORE_SERVICE_CREDENTIALS in compose,
// without its indentation.
func serviceCredentialsComposeLine(t *testing.T) string {
	t.Helper()
	raw, err := os.ReadFile(serviceCredentialsComposePath)
	if err != nil {
		t.Fatalf("read %s: %v", serviceCredentialsComposePath, err)
	}
	lines := strings.Split(strings.ReplaceAll(string(raw), "\r\n", "\n"), "\n")
	found := -1
	for index, line := range lines {
		if strings.TrimSpace(line) != "SESSION_CORE_SERVICE_CREDENTIALS: >-" {
			continue
		}
		if found >= 0 {
			t.Fatalf("SESSION_CORE_SERVICE_CREDENTIALS appears more than once in %s", serviceCredentialsComposePath)
		}
		found = index
	}
	if found < 0 || found+1 >= len(lines) {
		t.Fatalf("SESSION_CORE_SERVICE_CREDENTIALS block not found in %s", serviceCredentialsComposePath)
	}
	return strings.TrimSpace(lines[found+1])
}

// renderServiceCredentialsCompose substitutes the compose variables on that line the way compose does for
// this stack. A variable set in env renders as its value. An unset ${NAME:-default} renders as its default.
// An unset ${NAME:?message} would abort compose, so it fails the test. Any other variable also fails the
// test, so a new one has to be reviewed here first.
func renderServiceCredentialsCompose(t *testing.T, line string, env map[string]string) string {
	t.Helper()
	return composeVariable.ReplaceAllStringFunc(line, func(reference string) string {
		match := composeVariable.FindStringSubmatch(reference)
		name, modifier := match[1], match[2]
		if name != "SESSION_CORE_SERVICE_TOKEN" {
			t.Fatalf("SESSION_CORE_SERVICE_CREDENTIALS references %s, which this test does not know", name)
		}
		if value := env[name]; value != "" {
			return value
		}
		if strings.HasPrefix(modifier, ":-") {
			return strings.TrimPrefix(modifier, ":-")
		}
		t.Fatalf("compose would abort: %s is unset and has no default", name)
		return ""
	})
}

// loadComposeServiceCredentials sets SESSION_CORE_SERVICE_CREDENTIALS to the registry rendered with env and runs the
// real loader. It returns the entries kept and everything the loader logged.
func loadComposeServiceCredentials(t *testing.T, env map[string]string) ([]serviceCredential, string) {
	t.Helper()
	return loadServiceCredentialsWithLog(t, renderServiceCredentialsCompose(t, serviceCredentialsComposeLine(t), env))
}

// loadServiceCredentialsWithLog sets SESSION_CORE_SERVICE_CREDENTIALS to raw and runs the real loader. It returns the
// entries kept and everything the loader logged. The zerolog global logger is pointed at a buffer for the test and
// restored when the test ends.
func loadServiceCredentialsWithLog(t *testing.T, raw string) ([]serviceCredential, string) {
	t.Helper()
	t.Setenv("SESSION_CORE_SERVICE_CREDENTIALS", raw)

	var logged bytes.Buffer
	previous := log.Logger
	log.Logger = zerolog.New(&logged)
	t.Cleanup(func() { log.Logger = previous })

	credentials := loadServiceCredentials()
	return credentials, logged.String()
}

// principalsOf lists the entries the loader kept as principal[scopes], for comparisons and messages.
func principalsOf(credentials []serviceCredential) []string {
	summary := make([]string, 0, len(credentials))
	for _, credential := range credentials {
		summary = append(summary, credential.Principal+"["+strings.Join(credential.Scopes, ",")+"]")
	}
	return summary
}

// TestServiceCredentialsComposeDefaultBootIsSilent is the switch-off boot. compose writes the registry with the gateway
// credential alone, so session-core keeps verevon-gateway, logs nothing, and boots as it did before P2-cp-07.
func TestServiceCredentialsComposeDefaultBootIsSilent(t *testing.T) {
	credentials, logged := loadComposeServiceCredentials(t, map[string]string{
		"SESSION_CORE_SERVICE_TOKEN": composeFixtureGatewayToken,
	})
	if logged != "" {
		t.Fatalf("the default boot must log nothing, got: %s", logged)
	}
	if got, want := strings.Join(principalsOf(credentials), " "), "verevon-gateway[sessions:read,sessions:write]"; got != want {
		t.Fatalf("the default boot keeps %q, want %q", got, want)
	}
	if credentials[0].Token != composeFixtureGatewayToken {
		t.Fatal("the verevon-gateway token changed while loading the registry")
	}
}

// orgCoreConsumerEnv returns org-core's consumerEnv from the principal registry. That is the service API key org-core
// presents to Auth Core, from which it mints the session-core token for its thread receipt.
func orgCoreConsumerEnv(t *testing.T) string {
	t.Helper()
	raw, err := os.ReadFile(servicePrincipalsPath)
	if err != nil {
		t.Fatalf("read %s: %v", servicePrincipalsPath, err)
	}
	var principals map[string]json.RawMessage
	if err := json.Unmarshal(raw, &principals); err != nil {
		t.Fatalf("parse %s: %v", servicePrincipalsPath, err)
	}
	var orgCore struct {
		ConsumerEnv string `json:"consumerEnv"`
	}
	if err := json.Unmarshal(principals["org-core"], &orgCore); err != nil {
		t.Fatalf("org-core in %s: %v", servicePrincipalsPath, err)
	}
	return orgCore.ConsumerEnv
}

// TestServiceCredentialsComposeHoldsNoOrgCoreEntry pins the fix. org-core reads its thread receipt with a token Auth Core
// mints from its service API key, so the static registry carries no org-core entry. That key is org-core's consumerEnv,
// and compose must never hand it to session-core. The stale ORG_CORE_SESSION_BEARER name must not come back either.
func TestServiceCredentialsComposeHoldsNoOrgCoreEntry(t *testing.T) {
	consumer := orgCoreConsumerEnv(t)
	if consumer != "ORG_CORE_SESSION_SERVICE_API_KEY" {
		t.Fatalf("org-core's consumerEnv is %q, want ORG_CORE_SESSION_SERVICE_API_KEY", consumer)
	}
	if line := serviceCredentialsComposeLine(t); strings.Contains(line, "org-core") || strings.Contains(line, "${"+consumer) {
		t.Fatalf("SESSION_CORE_SERVICE_CREDENTIALS must not carry org-core or ${%s}: the receipt uses a minted token, not a static bearer", consumer)
	}
	raw, err := os.ReadFile(serviceCredentialsComposePath)
	if err != nil {
		t.Fatalf("read %s: %v", serviceCredentialsComposePath, err)
	}
	if strings.Contains(string(raw), "ORG_CORE_SESSION_BEARER") {
		t.Fatal("compose references ORG_CORE_SESSION_BEARER, a credential name that nothing defines")
	}
}

// TestServiceCredentialsEmptyAndShortTokensAreDropped pins the loader's two rules without compose. An empty token is an
// unset slot: it is dropped without a log line. A non-empty token that is short is an invalid entry: it is logged once
// and dropped.
func TestServiceCredentialsEmptyAndShortTokensAreDropped(t *testing.T) {
	raw := `[{"principal":"verevon-gateway","audience":"session-core","token":"` + composeFixtureGatewayToken +
		`","scopes":["sessions:read","sessions:write"]},{"principal":"fixture-empty","audience":"session-core","token":"",` +
		`"scopes":["sessions:read"]},{"principal":"fixture-short","audience":"session-core","token":"fixture-short",` +
		`"scopes":["sessions:read"]}]`
	credentials, logged := loadServiceCredentialsWithLog(t, raw)
	if n := strings.Count(logged, "invalid entry ignored"); n != 1 || !strings.Contains(logged, `"principal":"fixture-short"`) {
		t.Fatalf("a short non-empty token must be logged as an invalid entry exactly once, logged %d times: %s", n, logged)
	}
	if strings.Contains(logged, "fixture-empty") {
		t.Fatalf("an empty token must be dropped without a log line, got: %s", logged)
	}
	if got, want := strings.Join(principalsOf(credentials), " "), "verevon-gateway[sessions:read,sessions:write]"; got != want {
		t.Fatalf("session-core keeps %q, want %q", got, want)
	}
}
