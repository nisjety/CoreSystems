package config

import (
	"strings"
	"testing"
)

// Both Doffin keys are sent to whatever host DOFFIN_BASE_URL names, so a
// poisoned variable would hand them over. Any host other than the real one has
// to be confirmed by name.
func TestTheBaseURLHostMustBeConfirmedUnlessItIsDoffin(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		vars    map[string]string
		wantErr bool
	}{
		{"the default", nil, false},
		{"the real host with another path", map[string]string{"DOFFIN_BASE_URL": "https://api.doffin.no/public/v3"}, false},
		{"the real host in capitals", map[string]string{"DOFFIN_BASE_URL": "https://API.DOFFIN.NO/public/v2"}, false},
		{"loopback for a local mock", map[string]string{"DOFFIN_BASE_URL": "http://127.0.0.1:9000/public/v2"}, false},
		{"localhost for a local mock", map[string]string{"DOFFIN_BASE_URL": "http://localhost:9000/x"}, false},
		{"another host, unconfirmed", map[string]string{"DOFFIN_BASE_URL": "https://evil.example/public/v2"}, true},
		{"a look-alike host", map[string]string{"DOFFIN_BASE_URL": "https://api.doffin.no.evil.example/public/v2"}, true},
		{"another host, confirmed by name", map[string]string{"DOFFIN_BASE_URL": "https://api-test.example.no/public/v2", "DOFFIN_BASE_URL_ALLOW_HOST": "api-test.example.no"}, false},
		{"another host, confirmed with the wrong name", map[string]string{"DOFFIN_BASE_URL": "https://evil.example/public/v2", "DOFFIN_BASE_URL_ALLOW_HOST": "api.doffin.no"}, true},
		{"a look-alike with a loopback prefix", map[string]string{"DOFFIN_BASE_URL": "http://127.0.0.1.evil.example/x"}, true},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			_, err := LoadDoffinFrom(env(with(map[string]string{"DOFFIN_API_KEY": primaryKey}, tc.vars)))
			if (err != nil) != tc.wantErr {
				t.Fatalf("error = %v, wantErr %v", err, tc.wantErr)
			}
			if err != nil {
				for _, name := range []string{"DOFFIN_BASE_URL", "DOFFIN_BASE_URL_ALLOW_HOST"} {
					if !strings.Contains(err.Error(), name) {
						t.Errorf("the error should name %s: %q", name, err)
					}
				}
			}
		})
	}
}

func TestTheServiceEnforcesTheSameBaseURLRule(t *testing.T) {
	t.Parallel()

	_, err := LoadFrom(env(with(required(), map[string]string{
		"DOFFIN_API_KEY":  primaryKey,
		"DOFFIN_BASE_URL": "https://evil.example/public/v2",
	})))
	if err == nil || !strings.Contains(err.Error(), "DOFFIN_BASE_URL") {
		t.Fatalf("error = %v, want one naming DOFFIN_BASE_URL", err)
	}
}

// The URL may carry credentials; the message names the host only.
func TestTheBaseURLErrorNeverEchoesCredentialsFromTheURL(t *testing.T) {
	t.Parallel()

	_, err := LoadDoffinFrom(env(map[string]string{
		"DOFFIN_API_KEY":  primaryKey,
		"DOFFIN_BASE_URL": "https://admin:hunter2-secret@evil.example/public/v2?token=abc123",
	}))
	if err == nil {
		t.Fatal("expected an error")
	}
	for _, leak := range []string{"hunter2-secret", "admin", "abc123", primaryKey} {
		if strings.Contains(err.Error(), leak) {
			t.Fatalf("the error leaked %q: %s", leak, err)
		}
	}
}
