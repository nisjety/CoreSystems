package main

import (
	"bytes"
	"strings"
	"testing"
)

// H8 and H3 through the CLI: cli takes its environment as a parameter, so a test
// can pass one in. A remote DSN with a plaintext sslmode is refused with exit 1
// before any database or Doffin work, and the output names ANBUD_DSN without
// echoing the DSN. The test needs no database and makes no network call.
func TestCLIRefusesPlaintextRemoteDSN(t *testing.T) {
	const password = "cli-refusal-marker-pw"
	env := map[string]string{
		"ANBUD_DSN":       "postgres://anbud:" + password + "@db.example.com:5432/anbud?sslmode=prefer",
		"DOFFIN_API_KEY":  "cli-refusal-key-0001",
		"DOFFIN_BASE_URL": "http://127.0.0.1:9/public/v2",
	}
	var stdout, stderr bytes.Buffer
	code := cli([]string{"-from", "2020-01-01", "-to", "2020-01-02"}, mapEnv(env), &stdout, &stderr)

	if code != exitUsage {
		t.Fatalf("exit = %d, want %d (usage)", code, exitUsage)
	}
	if stdout.Len() != 0 {
		t.Errorf("stdout = %q, want nothing: no run should have started", stdout.String())
	}
	if !strings.Contains(stderr.String(), "ANBUD_DSN") {
		t.Errorf("stderr %q does not name ANBUD_DSN", stderr.String())
	}
	out := stdout.String() + stderr.String()
	for _, secret := range []string{password, "db.example.com", "cli-refusal-key-0001"} {
		if strings.Contains(out, secret) {
			t.Errorf("output echoes %q", secret)
		}
	}
}

// A missing ANBUD_DSN is a usage error that names the variable, and nothing runs.
func TestCLIRequiresDSNByName(t *testing.T) {
	env := map[string]string{
		"DOFFIN_API_KEY":  "cli-refusal-key-0002",
		"DOFFIN_BASE_URL": "http://127.0.0.1:9/public/v2",
	}
	var stdout, stderr bytes.Buffer
	code := cli([]string{"-from", "2020-01-01", "-to", "2020-01-02"}, mapEnv(env), &stdout, &stderr)

	if code != exitUsage {
		t.Fatalf("exit = %d, want %d (usage)", code, exitUsage)
	}
	if !strings.Contains(stderr.String(), "ANBUD_DSN") {
		t.Errorf("stderr %q does not name ANBUD_DSN", stderr.String())
	}
}

// mapEnv reads a test environment from a map, as os.Getenv would read the process one.
func mapEnv(env map[string]string) func(string) string {
	return func(key string) string { return env[key] }
}
