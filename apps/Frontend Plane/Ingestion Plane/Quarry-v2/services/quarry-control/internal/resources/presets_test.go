package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func newPresetServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountPresets(r)
	MountJobs(r, db)
	return r, db
}

func TestPresets_ListReturnsCatalog(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	req := httptest.NewRequest(http.MethodGet, "/v1/presets", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	var env map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	data, _ := env["data"].(map[string]any)
	items, _ := data["items"].([]any)
	if len(items) < 4 {
		t.Fatalf("expected 4+ presets in catalog, got %d", len(items))
	}
	names := map[string]bool{}
	for _, it := range items {
		m, _ := it.(map[string]any)
		n, _ := m["name"].(string)
		names[n] = true
	}
	for _, want := range []string{"fast", "polite", "careful", "deterministic"} {
		if !names[want] {
			t.Errorf("preset %q missing from catalog", want)
		}
	}
}

func TestPresets_ListHasCarefulAndStealthAlias(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	req := httptest.NewRequest(http.MethodGet, "/v1/presets", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	var env map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	data, _ := env["data"].(map[string]any)
	items, _ := data["items"].([]any)
	byName := map[string]map[string]any{}
	for _, it := range items {
		m, _ := it.(map[string]any)
		n, _ := m["name"].(string)
		byName[n] = m
	}
	careful, ok := byName["careful"]
	if !ok {
		t.Fatal("careful missing from the catalog")
	}
	if _, has := careful["alias_of"]; has {
		t.Errorf("careful is the preset, not an alias: %v", careful["alias_of"])
	}
	if _, has := careful["deprecated"]; has {
		t.Errorf("careful must not be marked deprecated")
	}
	stealth, ok := byName["stealth"]
	if !ok {
		t.Fatal("the stealth alias must stay in the catalog for one release")
	}
	if stealth["alias_of"] != "careful" || stealth["deprecated"] != true {
		t.Errorf("stealth must be a deprecated alias of careful, got alias_of=%v deprecated=%v", stealth["alias_of"], stealth["deprecated"])
	}
	cp, _ := json.Marshal(careful["policy"])
	sp, _ := json.Marshal(stealth["policy"])
	if string(cp) != string(sp) {
		t.Errorf("alias policy differs from the preset's:\n careful=%s\n stealth=%s", cp, sp)
	}
	// Nothing in the public catalog sells evasion any more.
	for name, m := range byName {
		blob, _ := json.Marshal(map[string]any{"title": m["title"], "description": m["description"], "tags": m["tags"]})
		if name == "stealth" {
			continue // the alias must keep its old id; its own text is the deprecation notice
		}
		for _, banned := range []string{"stealth", "anti-bot", "evade", "undetect"} {
			if strings.Contains(strings.ToLower(string(blob)), banned) {
				t.Errorf("preset %q text mentions %q: %s", name, banned, blob)
			}
		}
	}
}

func TestPresets_GetByName(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	req := httptest.NewRequest(http.MethodGet, "/v1/presets/careful", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	if got := w.Header().Get("Deprecation"); got != "" {
		t.Errorf("careful must not carry a Deprecation header, got %q", got)
	}
	var env map[string]any
	_ = json.Unmarshal(w.Body.Bytes(), &env)
	data, _ := env["data"].(map[string]any)
	if data["name"] != "careful" {
		t.Errorf("expected name=careful, got %v", data["name"])
	}
	policy, _ := data["policy"].(map[string]any)
	if policy == nil {
		t.Fatal("policy missing from preset response")
	}
}

func TestPresets_StealthAliasResolvesToCarefulWithDeprecation(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	req := httptest.NewRequest(http.MethodGet, "/v1/presets/stealth", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	if got := w.Header().Get("Deprecation"); got != "true" {
		t.Errorf("Deprecation header = %q, want true", got)
	}
	if got := w.Header().Get("Link"); got != "</v1/presets/careful>; rel=\"successor-version\"" {
		t.Errorf("Link header = %q", got)
	}
	var env map[string]any
	_ = json.Unmarshal(w.Body.Bytes(), &env)
	data, _ := env["data"].(map[string]any)
	if data["name"] != "stealth" || data["alias_of"] != "careful" || data["deprecated"] != true {
		t.Errorf("unexpected alias body: %v", data)
	}

	old, ok := ResolvePreset("stealth")
	if !ok {
		t.Fatal("ResolvePreset(stealth) must keep resolving")
	}
	neu, ok := ResolvePreset("careful")
	if !ok {
		t.Fatal("ResolvePreset(careful) not found")
	}
	a, _ := json.Marshal(old)
	b, _ := json.Marshal(neu)
	if string(a) != string(b) {
		t.Errorf("alias and preset resolve to different policies:\n%s\n%s", a, b)
	}
}

// The Block policy of careful is recorded, not enforced: no Rust or Go reader
// acts on Block.On / Block.Action. The description must say so, so an API
// caller is not told that a challenge ends the run when it does not.
func TestPresets_CarefulBlockIsAdvisoryAndSaysSo(t *testing.T) {
	t.Parallel()
	p, ok := ResolvePreset("careful")
	if !ok {
		t.Fatal("careful preset not found")
	}
	var desc string
	for _, preset := range defaultPresets() {
		if preset.Name == "careful" {
			desc = preset.Description
		}
	}
	if desc == "" {
		t.Fatal("careful has no description in the catalog")
	}
	if strings.Contains(desc, "ends the run") || strings.Contains(desc, "abort the run") {
		t.Errorf("description claims enforcement nothing provides: %q", desc)
	}
	if !strings.Contains(desc, "advisory") {
		t.Errorf("description must say the Block policy is advisory: %q", desc)
	}
	if p.Block.Action != quarrycontracts.BlkAbort {
		t.Errorf("the recorded Block action changed: %+v", p.Block)
	}
}

func TestPresets_CarefulAbortsOnChallenge(t *testing.T) {
	t.Parallel()
	p, ok := ResolvePreset("careful")
	if !ok {
		t.Fatal("careful preset not found")
	}
	if p.Block.On != quarrycontracts.TrigChallenge || p.Block.Action != quarrycontracts.BlkAbort {
		t.Errorf("careful must abort on a challenge, got %+v", p.Block)
	}
	// Pacing and the sticky proxy are what the preset always did; renaming
	// must not change them.
	if p.Concurrency.PerRun != 4 || p.Concurrency.PerDomain != 1 {
		t.Errorf("concurrency changed: %+v", p.Concurrency)
	}
	if p.Delay.MinMs != 1500 || p.Delay.MaxMs != 5000 || !p.Delay.Jitter {
		t.Errorf("delay changed: %+v", p.Delay)
	}
	if p.Proxy.Strategy != quarrycontracts.ProxySticky {
		t.Errorf("proxy strategy changed: %+v", p.Proxy)
	}
	if p.Robots != quarrycontracts.RobotsRespect {
		t.Errorf("robots changed: %v", p.Robots)
	}
}

func TestPresets_GetUnknownReturns404(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	req := httptest.NewRequest(http.MethodGet, "/v1/presets/does-not-exist", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", w.Code)
	}
}

func TestCreateJob_AppliesPresetPolicy(t *testing.T) {
	t.Parallel()
	// The new id and the deprecated alias stamp the same policy on the job.
	for _, preset := range []string{"careful", "stealth"} {
		h, _ := newPresetServer(t)
		body := `{"kind":"scrape","preset":"` + preset + `","params":{"url":"https://x"}}`
		req := httptest.NewRequest(http.MethodPost, "/v1/jobs?org_id=org_test", strings.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
		w := httptest.NewRecorder()
		h.ServeHTTP(w, req)
		if w.Code != http.StatusCreated {
			t.Fatalf("%s: status = %d; body=%s", preset, w.Code, w.Body.String())
		}
		var env map[string]any
		_ = json.Unmarshal(w.Body.Bytes(), &env)
		data, _ := env["data"].(map[string]any)
		policy, _ := data["policy"].(map[string]any)
		concurrency, _ := policy["concurrency"].(map[string]any)
		if concurrency["per_run"].(float64) != 4 {
			t.Errorf("%s: expected per_run=4, got %v", preset, concurrency["per_run"])
		}
		block, _ := policy["block"].(map[string]any)
		if block["on"] != "challenge" || block["action"] != "abort" {
			t.Errorf("%s: expected block challenge/abort, got %v", preset, block)
		}
	}
}

func TestCreateJob_RejectsUnknownPreset(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	body := `{"kind":"scrape","preset":"nosuch","params":{"url":"https://x"}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs?org_id=org_test", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400; body=%s", w.Code, w.Body.String())
	}
}

func TestResolvePreset_DirectLookup(t *testing.T) {
	t.Parallel()
	p, ok := ResolvePreset("polite")
	if !ok {
		t.Fatal("polite preset not found")
	}
	def := quarrycontracts.DefaultRunPolicy()
	if p.Concurrency.PerRun != def.Concurrency.PerRun {
		t.Errorf("polite preset diverges from default policy: %+v vs %+v", p, def)
	}
}

// baselineExcess lists the web-policy keys a preset's policy needs above the
// baseline (DefaultRunPolicy). A robots mode more permissive than respect needs
// robots.max_mode=ignore; any proxy strategy other than none needs a pooled
// proxy tier, proxy.max_tier=pool (the baseline proxy tier is none).
func baselineExcess(p quarrycontracts.RunPolicy) []string {
	var out []string
	if p.Robots == quarrycontracts.RobotsIgnore {
		out = append(out, "robots.max_mode=ignore")
	}
	if p.Proxy.Strategy != quarrycontracts.ProxyNone {
		out = append(out, "proxy.max_tier=pool")
	}
	return out
}

func requiresContain(list []string, want string) bool {
	for _, v := range list {
		if v == want {
			return true
		}
	}
	return false
}

// WS7.F: every preset whose policy exceeds the baseline declares the excess in
// requires, and every declared requirement is one its policy really exceeds.
func TestEveryPresetThatExceedsTheBaselineDeclaresRequires(t *testing.T) {
	t.Parallel()
	if got := baselineExcess(quarrycontracts.DefaultRunPolicy()); len(got) != 0 {
		t.Fatalf("the baseline policy exceeds the baseline: %v", got)
	}
	exceeding := 0
	for _, p := range defaultPresets() {
		want := baselineExcess(p.Policy)
		if len(want) > 0 {
			exceeding++
		}
		for _, w := range want {
			if !requiresContain(p.Requires, w) {
				t.Errorf("preset %q exceeds the baseline (%s) but requires is %v", p.Name, w, p.Requires)
			}
		}
		for _, r := range p.Requires {
			if !requiresContain(want, r) {
				t.Errorf("preset %q declares %q, which its policy does not exceed", p.Name, r)
			}
		}
	}
	// Guard against a vacuous pass: the catalogue has exceeding presets today.
	if exceeding == 0 {
		t.Fatal("no preset exceeds the baseline; the requires check would pass vacuously")
	}
}

// WS7.F: requires is on the wire for presets that declare it, and absent for
// the baseline preset (omitempty).
func TestPresets_RequiresOnTheWire(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	cases := map[string][]any{
		"fast":   {"robots.max_mode=ignore"},
		"polite": nil,
	}
	for name, want := range cases {
		req := httptest.NewRequest(http.MethodGet, "/v1/presets/"+name, nil)
		w := httptest.NewRecorder()
		h.ServeHTTP(w, req)
		if w.Code != http.StatusOK {
			t.Fatalf("%s: status = %d; body=%s", name, w.Code, w.Body.String())
		}
		var env map[string]any
		if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
			t.Fatal(err)
		}
		data, _ := env["data"].(map[string]any)
		got, has := data["requires"]
		if want == nil {
			if has {
				t.Errorf("%s: requires must be absent, got %v", name, got)
			}
			continue
		}
		list, _ := got.([]any)
		if len(list) != len(want) || (len(list) > 0 && list[0] != want[0]) {
			t.Errorf("%s: requires = %v, want %v", name, got, want)
		}
	}
}
