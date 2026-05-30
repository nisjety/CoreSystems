package scraper

import (
	"testing"
)

func TestDetectSPASignals_EmptyHTML(t *testing.T) {
	signals := DetectSPASignals("")
	if len(signals) != 0 {
		t.Errorf("expected 0 signals for empty HTML, got %d", len(signals))
	}
}

func TestDetectSPASignals_StaticHTML(t *testing.T) {
	html := `<html><head><title>Test</title></head><body>
		<h1>Hello World</h1>
		<p>This is a real page with plenty of text content that should not trigger
		any SPA detection. It has paragraphs, headings, and no JavaScript frameworks.
		The quick brown fox jumps over the lazy dog. Lorem ipsum dolor sit amet.</p>
	</body></html>`

	signals := DetectSPASignals(html)
	if NeedsJSRendering(html) {
		t.Errorf("static HTML should not need JS rendering, signals: %+v", signals)
	}
}

func TestDetectSPASignals_ReactSPA(t *testing.T) {
	html := `<!DOCTYPE html><html><head>
		<script src="/static/js/main.abc123.js"></script>
	</head><body>
		<noscript>You need to enable JavaScript to run this app.</noscript>
		<div id="root"></div>
	</body></html>`

	signals := DetectSPASignals(html)
	if len(signals) == 0 {
		t.Fatal("expected SPA signals for React app")
	}

	if !NeedsJSRendering(html) {
		t.Error("React SPA should need JS rendering")
	}

	// Verify specific signals are detected
	found := make(map[string]bool)
	for _, s := range signals {
		found[s.Name] = true
	}
	if !found["empty-root-container"] {
		t.Error("expected empty-root-container signal")
	}
	if !found["noscript-enable-js"] {
		t.Error("expected noscript-enable-js signal")
	}
	if !found["content-thin"] {
		t.Error("expected content-thin signal")
	}
}

func TestDetectSPASignals_NextJS(t *testing.T) {
	html := `<!DOCTYPE html><html><head>
		<script id="__NEXT_DATA__" type="application/json">{"props":{}}</script>
	</head><body>
		<div id="__next"></div>
	</body></html>`

	if !NeedsJSRendering(html) {
		t.Error("Next.js SPA should need JS rendering")
	}
}

func TestDetectSPASignals_VueNuxt(t *testing.T) {
	html := `<!DOCTYPE html><html><head></head><body>
		<div id="__nuxt"></div>
		<script>window.__NUXT__={}</script>
	</body></html>`

	if !NeedsJSRendering(html) {
		t.Error("Nuxt SPA should need JS rendering")
	}
}

func TestDetectSPASignals_Angular(t *testing.T) {
	html := `<!DOCTYPE html><html ng-version="17.0.0"><head></head><body>
		<app-root></app-root>
	</body></html>`

	signals := DetectSPASignals(html)
	found := false
	for _, s := range signals {
		if s.Name == "angular" {
			found = true
		}
	}
	if !found {
		t.Errorf("expected angular signal, got %+v", signals)
	}
}

func TestDetectSPASignals_HeavyJS(t *testing.T) {
	html := `<!DOCTYPE html><html><head>
		<script src="a.js"></script>
		<script src="b.js"></script>
		<script src="c.js"></script>
		<script src="d.js"></script>
		<script src="e.js"></script>
		<script src="f.js"></script>
	</head><body>
		<div id="app"></div>
	</body></html>`

	signals := DetectSPASignals(html)
	found := make(map[string]bool)
	for _, s := range signals {
		found[s.Name] = true
	}
	if !found["heavy-js-few-paragraphs"] {
		t.Error("expected heavy-js-few-paragraphs signal")
	}
}

func TestDetectSPASignals_Gatsby(t *testing.T) {
	html := `<!DOCTYPE html><html><head></head><body>
		<div id="___gatsby"></div>
	</body></html>`

	signals := DetectSPASignals(html)
	found := false
	for _, s := range signals {
		if s.Name == "gatsby" {
			found = true
		}
	}
	if !found {
		t.Errorf("expected gatsby signal, got %+v", signals)
	}
}

func TestExtractVisibleText(t *testing.T) {
	html := `<html><body>
		<script>var x = 1;</script>
		<style>.foo { color: red; }</style>
		<p>Hello World</p>
		<div>More content here</div>
	</body></html>`

	text := extractVisibleText(html)
	if len(text) == 0 {
		t.Error("expected non-empty visible text")
	}
	if !contains(text, "Hello World") {
		t.Errorf("expected 'Hello World' in visible text, got: %s", text)
	}
	if contains(text, "var x") {
		t.Error("script content should be stripped")
	}
	if contains(text, "color: red") {
		t.Error("style content should be stripped")
	}
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(s) > 0 && containsStr(s, substr))
}

func containsStr(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

// ─── DetectSPA (framework name resolution) ──────────────────────────────────

func TestDetectSPA_NextJS_Framework(t *testing.T) {
	html := `<!DOCTYPE html><html><head>
		<script id="__NEXT_DATA__" type="application/json">{"props":{}}</script>
	</head><body><div id="__next"></div></body></html>`

	result := DetectSPA(html)
	if result.Framework != "nextjs" {
		t.Errorf("expected framework=nextjs, got %q", result.Framework)
	}
}

func TestDetectSPA_React_Framework(t *testing.T) {
	// Plain React without __NEXT_DATA__ → react (not nextjs)
	html := `<!DOCTYPE html><html><head></head><body>
		<div id="root" data-reactroot=""></div>
		<noscript>You need to enable JavaScript to run this app.</noscript>
	</body></html>`

	result := DetectSPA(html)
	if result.Framework != "react" {
		t.Errorf("expected framework=react, got %q", result.Framework)
	}
}

func TestDetectSPA_Nuxt_Framework(t *testing.T) {
	html := `<!DOCTYPE html><html><head></head><body>
		<div id="__nuxt"></div>
		<script>window.__NUXT__={}</script>
	</body></html>`

	result := DetectSPA(html)
	if result.Framework != "nuxt" {
		t.Errorf("expected framework=nuxt, got %q", result.Framework)
	}
}

func TestDetectSPA_Angular_Framework(t *testing.T) {
	html := `<!DOCTYPE html><html ng-version="17.0.0"><head></head><body>
		<app-root></app-root>
	</body></html>`

	result := DetectSPA(html)
	if result.Framework != "angular" {
		t.Errorf("expected framework=angular, got %q", result.Framework)
	}
}

func TestDetectSPA_Astro_Framework(t *testing.T) {
	html := `<!DOCTYPE html><html><head></head><body>
		<astro-island uid="abc123" component-export="default"></astro-island>
		<p>Some server-rendered content from Astro.</p>
	</body></html>`

	result := DetectSPA(html)
	if result.Framework != "astro" {
		t.Errorf("expected framework=astro, got %q", result.Framework)
	}
}

func TestDetectSPA_Qwik_Framework(t *testing.T) {
	html := `<!DOCTYPE html><html><head></head><body>
		<div q:container="paused" q:version="1.2.3"></div>
	</body></html>`

	result := DetectSPA(html)
	if result.Framework != "qwik" {
		t.Errorf("expected framework=qwik, got %q", result.Framework)
	}
}

func TestDetectSPA_Remix_Framework(t *testing.T) {
	html := `<!DOCTYPE html><html><head></head><body>
		<script>window.__remixContext = {}</script>
	</body></html>`

	result := DetectSPA(html)
	if result.Framework != "remix" {
		t.Errorf("expected framework=remix, got %q", result.Framework)
	}
}

func TestDetectSPA_Gatsby_Framework(t *testing.T) {
	html := `<!DOCTYPE html><html><head></head><body>
		<div id="___gatsby"></div>
	</body></html>`

	result := DetectSPA(html)
	if result.Framework != "gatsby" {
		t.Errorf("expected framework=gatsby, got %q", result.Framework)
	}
}

func TestDetectSPA_SvelteKit_Framework(t *testing.T) {
	html := `<!DOCTYPE html><html><head></head><body>
		<div id="svelte"></div>
		<script>window.__sveltekit_data = {}</script>
	</body></html>`

	result := DetectSPA(html)
	if result.Framework != "sveltekit" {
		t.Errorf("expected framework=sveltekit, got %q (signals: %v)", result.Framework, result.Signals)
	}
}

func TestDetectSPA_Static_NoFramework(t *testing.T) {
	html := `<!DOCTYPE html><html><head><title>Static Page</title></head><body>
		<h1>Hello World</h1>
		<p>This is a plain static HTML page with no JavaScript framework magic.
		It has multiple paragraphs of real content. The quick brown fox jumps
		over the lazy dog. Lorem ipsum dolor sit amet consectetur adipiscing
		elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.</p>
	</body></html>`

	result := DetectSPA(html)
	if result.Framework != "" {
		t.Errorf("expected no framework for static HTML, got %q", result.Framework)
	}
	if result.NeedsJS {
		t.Error("static HTML should not need JS rendering")
	}
}

func TestDetectSPA_NeedsJS_TrueForEmptyRoot(t *testing.T) {
	html := `<!DOCTYPE html><html><head>
		<script src="/bundle.abc.js"></script>
	</head><body>
		<div id="root"></div>
		<noscript>You need JavaScript.</noscript>
	</body></html>`

	result := DetectSPA(html)
	if !result.NeedsJS {
		t.Error("page with empty root + noscript should need JS rendering")
	}
}

func TestDetectSPA_SignalsPopulated(t *testing.T) {
	html := `<!DOCTYPE html><html><head>
		<script id="__NEXT_DATA__" type="application/json">{"props":{}}</script>
	</head><body><div id="__next"></div></body></html>`

	result := DetectSPA(html)
	if len(result.Signals) == 0 {
		t.Error("DetectSPA should populate Signals slice")
	}
}
