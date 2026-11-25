package main

import (
	"os"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
)

// F-f10-15: SKILL_BUNDLE_RETENTION is effective only with SKILL_VERSIONS_API. The start-up warning names the closed
// reason exactly when retention was asked for and the coupling reads it as off.
func TestWarnBundleRetentionWithoutVersionsLogsOnlyTheMismatch(t *testing.T) {
	cases := []struct {
		name string
		env  map[string]string
		want int
	}{
		{name: "retention on, versions off", env: map[string]string{skillbundle.RetentionEnv: "true"}, want: 1},
		{name: "retention on, versions not the literal true", env: map[string]string{skillbundle.RetentionEnv: "true", "SKILL_VERSIONS_API": "1"}, want: 1},
		{name: "retention on, versions on", env: map[string]string{skillbundle.RetentionEnv: "true", "SKILL_VERSIONS_API": "true"}, want: 0},
		{name: "retention off, versions on", env: map[string]string{"SKILL_VERSIONS_API": "true"}, want: 0},
		{name: "retention unset", env: map[string]string{}, want: 0},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			logs := captureSlog(t)
			warned := warnBundleRetentionWithoutVersions(func(name string) string { return tc.env[name] })
			if got := strings.Count(logs.String(), bundleRetentionNeedsVersions); got != tc.want {
				t.Fatalf("warnings naming %s = %d, want %d; logs: %s", bundleRetentionNeedsVersions, got, tc.want, logs.String())
			}
			if warned != (tc.want == 1) {
				t.Fatalf("warnBundleRetentionWithoutVersions returned %v, want %v", warned, tc.want == 1)
			}
		})
	}
}

// The warning is called once, at construction in main, so a start logs it once and no request path logs it.
func TestBundleRetentionWarningIsCalledOnceInMain(t *testing.T) {
	raw, err := os.ReadFile("main.go")
	if err != nil {
		t.Fatalf("read main.go: %v", err)
	}
	if n := strings.Count(string(raw), "warnBundleRetentionWithoutVersions(os.Getenv)"); n != 1 {
		t.Fatalf("main.go calls warnBundleRetentionWithoutVersions %d times, want exactly 1 (at construction)", n)
	}
}
