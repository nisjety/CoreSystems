package providers

import (
	"slices"
	"testing"
)

func TestGoogleAnalyticsBundleRequestsOnlyReadAccess(t *testing.T) {
	provider := GoogleWorkspace()
	capabilities := ResolveCapabilities(provider, nil, []string{"analytics_readonly"})
	if !slices.Contains(capabilities, "analytics.read") || slices.Contains(capabilities, "ads.manage") {
		t.Fatalf("capabilities=%#v", capabilities)
	}
	scopes := ResolveScopes(provider, capabilities)
	if !slices.Contains(scopes, "https://www.googleapis.com/auth/analytics.readonly") {
		t.Fatalf("scopes=%#v", scopes)
	}
	if slices.Contains(scopes, "https://www.googleapis.com/auth/analytics.edit") {
		t.Fatal("analytics write scope requested")
	}
}
