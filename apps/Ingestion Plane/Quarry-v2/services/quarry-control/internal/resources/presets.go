package resources

import (
	"net/http"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
)

// Preset is a named, opinionated RunPolicy. The catalog is static — preset
// definitions ship with the binary and are not user-editable from the API.
// Phase 7: callers attach a preset by name to job creation; control resolves
// the policy and stamps it onto the job.
type Preset struct {
	Name        string   `json:"name"`
	Title       string   `json:"title"`
	Description string   `json:"description"`
	Tags        []string `json:"tags,omitempty"`
	// Requires names the web-policy capability keys (`key=value`, the names of
	// quarry_core::web_policy) that the policy needs above the baseline. WS7.F
	// declares them; job creation does not enforce them yet (WS7.C wires the
	// edge, WS7.I mints the claim that unlocks them).
	Requires []string                  `json:"requires,omitempty"`
	Policy   quarrycontracts.RunPolicy `json:"policy"`
	// AliasOf names the preset this entry is an alias of. An alias resolves to
	// the same policy; it exists so an id that shipped keeps working for one
	// release after a rename (WS3.I: stealth -> careful).
	AliasOf string `json:"alias_of,omitempty"`
	// Deprecated marks an alias that will be removed. GET /v1/presets/{name}
	// answers it with a Deprecation header and a successor Link.
	Deprecated bool `json:"deprecated,omitempty"`
}

// deprecatedPresetAliases maps a renamed preset id to its successor. Remove an
// entry (and the alias row in the catalog) one release after it ships.
var deprecatedPresetAliases = map[string]string{"stealth": "careful"}

// MountPresets registers `GET /v1/presets` (list) and
// `GET /v1/presets/{name}` (single).
func MountPresets(r chi.Router) {
	catalog := defaultPresets()
	r.Get("/v1/presets", func(w http.ResponseWriter, r *http.Request) {
		httpx.WriteJSON(w, r, http.StatusOK, map[string]any{
			"items": catalog,
		})
	})
	r.Get("/v1/presets/{name}", func(w http.ResponseWriter, r *http.Request) {
		name := chi.URLParam(r, "name")
		for _, p := range catalog {
			if p.Name == name {
				if p.Deprecated && p.AliasOf != "" {
					w.Header().Set("Deprecation", "true")
					w.Header().Set("Link", "</v1/presets/"+p.AliasOf+">; rel=\"successor-version\"")
				}
				httpx.WriteJSON(w, r, http.StatusOK, p)
				return
			}
		}
		httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "preset not found", map[string]any{"name": name})
	})
}

// ResolvePreset returns the named preset's policy or false if no such preset.
// Other resources (e.g. job creation) use this to materialize the policy.
func ResolvePreset(name string) (quarrycontracts.RunPolicy, bool) {
	for _, p := range defaultPresets() {
		if p.Name == name {
			return p.Policy, true
		}
	}
	return quarrycontracts.RunPolicy{}, false
}

func defaultPresets() []Preset {
	u32 := func(v uint32) *uint32 { return &v }
	presets := []Preset{
		{
			Name:        "fast",
			Title:       "Fast scrape",
			Description: "Aggressive concurrency, no delay, no jitter. For permissive APIs and partner sites.",
			Tags:        []string{"fast", "low-respect"},
			Requires:    []string{"robots.max_mode=ignore"},
			Policy: quarrycontracts.RunPolicy{
				Concurrency: quarrycontracts.Concurrency{PerRun: 32, PerDomain: 8},
				Delay:       quarrycontracts.Delay{MinMs: 0, MaxMs: 0, Jitter: false},
				Retry:       quarrycontracts.Retry{Max: 2, Backoff: quarrycontracts.BackoffFixed, BaseMs: 500},
				Proxy:       quarrycontracts.Proxy{Strategy: quarrycontracts.ProxyNone},
				Robots:      quarrycontracts.RobotsIgnore,
				Ordering:    quarrycontracts.OrdFifo,
				Block:       quarrycontracts.BlockPolicy{On: quarrycontracts.TrigChallenge, Action: quarrycontracts.BlkAbort},
				Checkpoint:  quarrycontracts.Checkpoint{EveryNPages: u32(200), EveryS: u32(120)},
				Determinism: quarrycontracts.DetOff,
			},
		},
		{
			Name:        "polite",
			Title:       "Polite scrape",
			Description: "Default ramp. Honors robots, modest delay with jitter.",
			Tags:        []string{"default", "polite"},
			Policy:      quarrycontracts.DefaultRunPolicy(),
		},
		{
			Name:        "careful",
			Title:       "Careful scrape",
			Description: "Slow and steady. Sticky proxy, longer jittered delays, robots respected. Quarry does not change identity to get past a challenge. The policy records Block {challenge, abort}, but no runtime reader enforces it yet: it is advisory, and a run is not stopped by a challenge.",
			Tags:        []string{"careful", "slow"},
			Requires:    []string{"proxy.max_tier=pool"},
			Policy: quarrycontracts.RunPolicy{
				Concurrency: quarrycontracts.Concurrency{PerRun: 4, PerDomain: 1},
				Delay:       quarrycontracts.Delay{MinMs: 1500, MaxMs: 5000, Jitter: true},
				Retry:       quarrycontracts.Retry{Max: 4, Backoff: quarrycontracts.BackoffExp, BaseMs: 2000},
				Proxy:       quarrycontracts.Proxy{Strategy: quarrycontracts.ProxySticky},
				Robots:      quarrycontracts.RobotsRespect,
				Ordering:    quarrycontracts.OrdFifo,
				Block: quarrycontracts.BlockPolicy{
					On:     quarrycontracts.TrigChallenge,
					Action: quarrycontracts.BlkAbort,
				},
				Checkpoint:  quarrycontracts.Checkpoint{EveryNPages: u32(25), EveryS: u32(30)},
				Determinism: quarrycontracts.DetBestEff,
			},
		},
		{
			Name:        "deterministic",
			Title:       "Deterministic capture",
			Description: "Strict determinism, single-threaded, robots strict. For audit and compliance use.",
			Tags:        []string{"deterministic", "audit"},
			Policy: quarrycontracts.RunPolicy{
				Concurrency: quarrycontracts.Concurrency{PerRun: 1, PerDomain: 1},
				Delay:       quarrycontracts.Delay{MinMs: 1000, MaxMs: 1000, Jitter: false},
				Retry:       quarrycontracts.Retry{Max: 0, Backoff: quarrycontracts.BackoffFixed, BaseMs: 0},
				Proxy:       quarrycontracts.Proxy{Strategy: quarrycontracts.ProxyNone},
				Robots:      quarrycontracts.RobotsStrict,
				Ordering:    quarrycontracts.OrdFifo,
				Block: quarrycontracts.BlockPolicy{
					On:     quarrycontracts.TrigChallenge,
					Action: quarrycontracts.BlkAbort,
				},
				Checkpoint:  quarrycontracts.Checkpoint{EveryNPages: u32(1), EveryS: u32(10)},
				Determinism: quarrycontracts.DetStrict,
			},
		},
	}
	// Deprecated aliases come last: same policy as the successor, own name.
	for old, successor := range deprecatedPresetAliases {
		for _, p := range presets {
			if p.Name != successor {
				continue
			}
			alias := p
			alias.Name = old
			alias.Description = "Deprecated alias of `" + successor + "`, kept for one release. " + p.Description
			alias.Tags = append([]string(nil), p.Tags...)
			alias.Requires = append([]string(nil), p.Requires...)
			alias.AliasOf = successor
			alias.Deprecated = true
			presets = append(presets, alias)
			break
		}
	}
	return presets
}
