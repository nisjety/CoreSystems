package main

import (
	"bytes"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

const runKey = "0123456789abcdef0123456789abcdef"

// These go through run, so they cover the wiring from the environment to the
// client. They set environment variables, so they cannot run in parallel.

func TestRunRejectsStrayArgumentsAndNamesTheRightFlag(t *testing.T) {
	err := run([]string{"-discover", "false"}, &bytes.Buffer{})
	if err == nil || !strings.Contains(err.Error(), "-discover=false") {
		t.Fatalf("error = %v, want one that writes -discover=false", err)
	}
}

func TestRunSendsTheSameKeyOnceWhenTheSecondaryIsIdentical(t *testing.T) {
	var (
		mu   sync.Mutex
		keys []string
	)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		keys = append(keys, r.Header.Get(doffin.KeyHeader))
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"numHitsTotal":1,"numHitsAccessible":1,"hits":[]}`)
	}))
	t.Cleanup(srv.Close)
	t.Setenv("DOFFIN_API_KEY", runKey)
	t.Setenv("DOFFIN_API_KEY_SECONDARY", runKey)
	t.Setenv("DOFFIN_BASE_URL", srv.URL+"/public/v2")

	var out bytes.Buffer
	if err := run([]string{"-discover=false"}, &out); err != nil {
		t.Fatalf("run: %v", err)
	}

	mu.Lock()
	defer mu.Unlock()
	if len(keys) != 1 || keys[0] != runKey {
		t.Fatalf("requests carried %d keys, want exactly one, the primary", len(keys))
	}
	text := out.String()
	for _, want := range []string{"identical", "Done.", "host: 127.0.0.1"} {
		if !strings.Contains(text, want) {
			t.Errorf("output should contain %q:\n%s", want, text)
		}
	}
	if strings.Contains(text, runKey) {
		t.Fatalf("the output printed a key:\n%s", text)
	}
}

func TestRunRefusesASecondaryKeyWithoutAPrimary(t *testing.T) {
	t.Setenv("DOFFIN_API_KEY", "")
	t.Setenv("DOFFIN_API_KEY_SECONDARY", runKey)

	err := run([]string{"-discover=false"}, &bytes.Buffer{})
	if err == nil || !strings.Contains(err.Error(), "DOFFIN_API_KEY_SECONDARY") {
		t.Fatalf("error = %v, want one that names DOFFIN_API_KEY_SECONDARY", err)
	}
	if strings.Contains(err.Error(), runKey) {
		t.Fatalf("the error echoed the key: %v", err)
	}
}

// Both keys go to whatever host DOFFIN_BASE_URL names, so an unconfirmed host
// is refused, and the URL, which may carry credentials, is never echoed.
func TestRunRefusesAnUnconfirmedHostWithoutEchoingIt(t *testing.T) {
	t.Setenv("DOFFIN_API_KEY", runKey)
	t.Setenv("DOFFIN_BASE_URL", "https://admin:hunter2-secret@evil.example/public/v2")
	t.Setenv("DOFFIN_BASE_URL_ALLOW_HOST", "")

	err := run([]string{"-discover=false"}, &bytes.Buffer{})
	if err == nil || !strings.Contains(err.Error(), "DOFFIN_BASE_URL_ALLOW_HOST") {
		t.Fatalf("error = %v, want one that names DOFFIN_BASE_URL_ALLOW_HOST", err)
	}
	for _, leak := range []string{"hunter2-secret", "admin", runKey} {
		if strings.Contains(err.Error(), leak) {
			t.Fatalf("the error leaked %q: %v", leak, err)
		}
	}
}
