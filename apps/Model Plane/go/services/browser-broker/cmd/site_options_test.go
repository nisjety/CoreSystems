package main

import (
	"testing"
	"time"

	bbserver "github.com/triodelab/model-plane/services/browser-broker/internal/server"
	"github.com/triodelab/model-plane/services/browser-broker/internal/sitepolicy"
)

func envOf(values map[string]string) func(string) string {
	return func(name string) string { return values[name] }
}

func TestSitePolicyOffNeedsNoOrgCoreAndMakesNoClient(t *testing.T) {
	options, err := serverOptionsFromEnv(envOf(nil))
	if err != nil {
		t.Fatalf("serverOptionsFromEnv: %v", err)
	}
	if options.SitePolicy != sitepolicy.ModeOff || options.SiteResolver != nil {
		t.Fatalf("off: mode=%q resolver=%v, want off with no client", options.SitePolicy, options.SiteResolver)
	}
}

func TestSitePolicyOnRequiresOrgCoreConfiguration(t *testing.T) {
	for _, mode := range []string{"shadow", "enforce", "typo"} {
		t.Run(mode, func(t *testing.T) {
			_, err := serverOptionsFromEnv(envOf(map[string]string{envSitePolicy: mode}))
			if err == nil {
				t.Fatal("a site policy with no org-core URL or token started")
			}
		})
	}
}

func TestSitePolicyOnBuildsTheClientWhenConfigured(t *testing.T) {
	options, err := serverOptionsFromEnv(envOf(map[string]string{
		envSitePolicy:   "enforce",
		envOrgCoreURL:   "http://org-core:8080",
		envOrgCoreToken: "test-token",
	}))
	if err != nil {
		t.Fatalf("serverOptionsFromEnv: %v", err)
	}
	if options.SitePolicy != sitepolicy.ModeEnforce || options.SiteResolver == nil {
		t.Fatalf("enforce: mode=%q resolver=%v", options.SitePolicy, options.SiteResolver)
	}
}

func TestGrantMaxTTLDefaultsAndClamps(t *testing.T) {
	cases := []struct {
		raw     string
		want    time.Duration
		wantErr bool
	}{
		{"", bbserver.DefaultMaxRequestTTL, false},
		{"45m", 45 * time.Minute, false},
		{"10s", bbserver.MinGrantTTL, false},
		{"2h", bbserver.MaxGrantTTL, false},
		{"thirty", 0, true},
	}
	for _, tc := range cases {
		got, err := grantMaxTTLFromEnv(envOf(map[string]string{envGrantMaxTTL: tc.raw}))
		if (err != nil) != tc.wantErr {
			t.Fatalf("%q: err = %v, wantErr %v", tc.raw, err, tc.wantErr)
		}
		if err == nil && got != tc.want {
			t.Fatalf("%q: got %v, want %v", tc.raw, got, tc.want)
		}
	}
}
