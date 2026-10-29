package providers

import "testing"

func TestSiteSourceProvidersAreReadOnlyManualNotOAuth(t *testing.T) {
	for _, key := range []string{"cookiebot"} {
		p, ok := Find(key)
		if !ok || p.AuthType != "api_key" || p.DirectOAuthReady || len(p.Capabilities) == 0 {
			t.Fatalf("manual provider missing: %s", key)
		}
		if _, ok := FindOAuth(key); ok {
			t.Fatal("manual API key provider offered OAuth")
		}
		for _, cap := range p.Capabilities {
			if cap.Direction != "read" {
				t.Fatal("write capability exposed")
			}
		}
	}
}

func TestAlgoliaUsesReadOnlyResourceBoundOAuth(t *testing.T) {
	p, ok := FindOAuth("algolia")
	if !ok || !p.DirectOAuthReady || p.AuthType != "oauth2_authorization_code_pkce" {
		t.Fatal("Algolia OAuth missing")
	}
	if len(p.Capabilities) != 1 || len(p.Capabilities[0].Scopes) != 1 || p.Capabilities[0].Scopes[0] != "public" || p.Capabilities[0].Direction != "read" {
		t.Fatal("non-read-only Algolia grant")
	}
}
