package driver

import (
	"strings"
	"testing"
)

var knownFrameworks = []string{
	"react", "nextjs",
	"vue", "nuxt",
	"angular",
	"svelte", "sveltekit",
	"gatsby",
	"remix",
	"astro",
	"qwik",
}

func TestFrameworkWaitJS_KnownFrameworks_NonEmpty(t *testing.T) {
	for _, fw := range knownFrameworks {
		js := FrameworkWaitJS(fw)
		if js == "" {
			t.Errorf("FrameworkWaitJS(%q) returned empty string", fw)
		}
	}
}

func TestFrameworkWaitJS_UnknownFramework_Empty(t *testing.T) {
	for _, fw := range []string{"", "unknown", "ember", "backbone"} {
		js := FrameworkWaitJS(fw)
		if js != "" {
			t.Errorf("FrameworkWaitJS(%q) expected empty string, got non-empty", fw)
		}
	}
}

func TestFrameworkWaitJS_ReturnsPromise(t *testing.T) {
	for _, fw := range knownFrameworks {
		js := FrameworkWaitJS(fw)
		if !strings.Contains(js, "Promise") {
			t.Errorf("FrameworkWaitJS(%q) must use Promise-based wait, got: %s", fw, js[:min(80, len(js))])
		}
	}
}

func TestFrameworkWaitJS_TimeoutFallback(t *testing.T) {
	// All framework JS snippets should have a timeout fallback so they don't
	// block forever if the framework never signals ready.
	for _, fw := range knownFrameworks {
		js := FrameworkWaitJS(fw)
		if !strings.Contains(js, "setTimeout") && !strings.Contains(js, "setInterval") {
			t.Errorf("FrameworkWaitJS(%q) must include a timeout fallback", fw)
		}
	}
}

func TestFrameworkWaitJS_FrameworkSpecificMarkers(t *testing.T) {
	cases := []struct {
		fw     string
		marker string
	}{
		{"react", "data-reactroot"},
		{"nextjs", "__next"},
		{"vue", "data-v-"},
		{"nuxt", "__NUXT__"},
		{"angular", "ng-version"},
		{"svelte", "svelte"},
		{"gatsby", "___gatsby"},
		{"remix", "__remixContext"},
		{"astro", "astro-island"},
		{"qwik", "q\\\\:container"},
	}

	for _, tc := range cases {
		js := FrameworkWaitJS(tc.fw)
		if !strings.Contains(js, tc.marker) {
			t.Errorf("FrameworkWaitJS(%q) should reference %q, got: %s",
				tc.fw, tc.marker, js[:min(120, len(js))])
		}
	}
}

// min is a small helper since Go < 1.21 doesn't have min in the stdlib.
func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
