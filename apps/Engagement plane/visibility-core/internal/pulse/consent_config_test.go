package pulse

import "testing"

func TestGoogleConsentURLsUseVerevonAndRegisteredCallbackDefaults(t *testing.T) {
	returnURL, callbackURL, err := ResolveGoogleConsentURLs("", "")
	if err != nil || returnURL != "http://localhost:5173/studio/pulse" || callbackURL != "http://localhost:3026/oauth/callback/google" {
		t.Fatalf("unexpected defaults: %q %q %v", returnURL, callbackURL, err)
	}
}

func TestGoogleConsentURLsAcceptExplicitCanonicalTrustedDestinations(t *testing.T) {
	for _, tc := range []struct{ returnURL, callbackURL string }{
		{"http://127.0.0.1:5173/studio/pulse", "http://127.0.0.1:33026/oauth/callback/google"},
		{"http://[::1]:5173/studio/pulse", "http://[::1]:3026/oauth/callback/google"},
		{"https://app.example.com/studio/pulse", "https://integrations.example.com/oauth/callback/google"},
	} {
		returnURL, callbackURL, err := ResolveGoogleConsentURLs(tc.returnURL, tc.callbackURL)
		if err != nil || returnURL != tc.returnURL || callbackURL != tc.callbackURL {
			t.Fatalf("trusted destinations rejected: %v", err)
		}
	}
}

func TestGoogleConsentURLsRejectUnsafeOrAmbiguousConfiguration(t *testing.T) {
	for _, raw := range []string{
		"http://public.example.com/studio/pulse", "http://localhost.evil.example:5173/studio/pulse",
		"http://0.0.0.0:5173/studio/pulse", "https://user@app.example.com/studio/pulse",
		"https://app.example.com/studio/pulse?return=evil", "https://app.example.com/studio/pulse?",
		"https://app.example.com/studio/pulse#fragment", "https://app.example.com/studio/pulse#",
		"https://app.example.com/studio/../pulse", "https://app.example.com//studio/pulse",
		"https://app.example.com/studio/%2e%2e/pulse", "https://app.example.com/studio\\pulse",
		"https://App.example.com/studio/pulse", "https://app.example.com:443/studio/pulse",
		"https://app.example.com/studio/pulse ", "https://app.example.com",
		"//app.example.com/studio/pulse", "/studio/pulse", "javascript:alert(1)",
	} {
		t.Run(raw, func(t *testing.T) {
			if _, _, err := ResolveGoogleConsentURLs(raw, ""); err == nil {
				t.Fatal("unsafe return URL accepted")
			}
			if _, _, err := ResolveGoogleConsentURLs("", raw); err == nil {
				t.Fatal("unsafe callback URL accepted")
			}
		})
	}
}
