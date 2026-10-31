package doffin

import (
	"context"
	"fmt"
	"net/http"
	"strings"
	"testing"
)

// The key is redacted from upstream text. If invisible characters were removed
// only afterwards, an upstream could hide the key from the redaction by putting
// one inside it, and the full key would then print.
func TestAnEchoedKeyCannotHideBehindInvisibleCharacters(t *testing.T) {
	t.Parallel()

	// The separators are written as escapes so no invisible character sits in
	// this file. A JSON escape in the body is decoded by extractMessage; a raw
	// character in the body arrives as it is.
	tests := map[string]string{
		"a raw zero-width space":          "​",
		"a JSON-escaped zero-width space": `​`,
		"a raw bidi override":             "‮",
		"a JSON-escaped soft hyphen":      `­`,
		"a JSON-escaped control":          `\u0007`,
	}
	for name, separator := range tests {
		t.Run(name, func(t *testing.T) {
			t.Parallel()

			var echoed strings.Builder
			for i, r := range testKey {
				if i > 0 && i%5 == 0 {
					echoed.WriteString(separator)
				}
				echoed.WriteRune(r)
			}
			c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.WriteHeader(http.StatusUnauthorized)
				_, _ = fmt.Fprintf(w, `{"message":"invalid key %s"}`, echoed.String())
			}))

			_, err := c.Search(context.Background(), SearchParams{})
			if err == nil {
				t.Fatal("expected an error")
			}
			if strings.Contains(err.Error(), testKey) {
				t.Fatalf("the key was printed in full: %q", err)
			}
			for _, fragment := range []string{testKey[:12], testKey[10:22], testKey[20:]} {
				if strings.Contains(err.Error(), fragment) {
					t.Fatalf("a large part of the key survived the redaction: %q", err)
				}
			}
			if !strings.Contains(err.Error(), redactedMarker) {
				t.Errorf("the redaction marker is missing, so the key was not recognised at all: %q", err)
			}
		})
	}
}

func TestIsLoopbackHost(t *testing.T) {
	t.Parallel()

	for host, want := range map[string]bool{
		"localhost":             true,
		"LOCALHOST":             true,
		"127.0.0.1":             true,
		"::1":                   true,
		"api.doffin.no":         false,
		"127.0.0.1.evil.com":    false,
		"localhost.evil.com":    false,
		"":                      false,
		"10.0.0.5":              false,
		"0.0.0.0":               false,
		"[::1]":                 false, // callers pass url.URL.Hostname(), which has no brackets
		"api.doffin.no.evil.io": false,
	} {
		if got := IsLoopbackHost(host); got != want {
			t.Errorf("IsLoopbackHost(%q) = %v, want %v", host, got, want)
		}
	}
	if DefaultHost != "api.doffin.no" {
		t.Errorf("DefaultHost = %q", DefaultHost)
	}
}
