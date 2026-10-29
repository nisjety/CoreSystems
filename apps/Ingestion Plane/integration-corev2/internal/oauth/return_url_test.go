package oauth

import (
	"testing"

	"github.com/triodelab/integration-corev2/internal/config"
)

func TestNormalizeReturnURLAllowsConfiguredExactOriginAndPath(t *testing.T) {
	cfg := config.Config{Environment: "production", OAuthAllowedReturnOrigins: []string{"https://app.verevon.test"}}
	destination, origin, err := cfg.NormalizeOAuthReturnURL("https://app.verevon.test/onboarding?step=integrations")
	if err != nil {
		t.Fatalf("NormalizeOAuthReturnURL error: %v", err)
	}
	if destination != "https://app.verevon.test/onboarding?step=integrations" || origin != "https://app.verevon.test" {
		t.Fatalf("destination/origin = %q/%q", destination, origin)
	}
}

func TestNormalizeReturnURLRejectsUntrustedOrAmbiguousDestinations(t *testing.T) {
	cfg := config.Config{Environment: "production", OAuthAllowedReturnOrigins: []string{"https://app.verevon.test"}}
	for _, destination := range []string{
		"https://evil.test/after-connect", "https://app.verevon.test.evil/after-connect", "//app.verevon.test/after-connect",
		"/after-connect", "javascript:alert(1)", "data:text/html,unsafe", "https://user:password@app.verevon.test/after-connect",
		"https://app.verevon.test/after-connect#fragment", "https://app.verevon.test/%zz",
	} {
		if _, _, err := cfg.NormalizeOAuthReturnURL(destination); err == nil {
			t.Errorf("NormalizeOAuthReturnURL(%q) succeeded, want rejection", destination)
		}
	}
}

func TestNormalizeReturnURLAllowsEmptyDestination(t *testing.T) {
	cfg := config.Config{Environment: "production", OAuthAllowedReturnOrigins: []string{"https://app.verevon.test"}}
	destination, origin, err := cfg.NormalizeOAuthReturnURL("")
	if err != nil || destination != "" || origin != "" {
		t.Fatalf("empty destination/origin/error = %q/%q/%v", destination, origin, err)
	}
}
