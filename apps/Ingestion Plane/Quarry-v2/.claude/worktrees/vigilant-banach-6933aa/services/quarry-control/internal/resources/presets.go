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
	Name        string                    `json:"name"`
	Title       string                    `json:"title"`
	Description string                    `json:"description"`
	Tags        []string                  `json:"tags,omitempty"`
	Policy      quarrycontracts.RunPolicy `json:"policy"`
}

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
	return []Preset{
		{
			Name:        "fast",
			Title:       "Fast scrape",
			Description: "Aggressive concurrency, no delay, no jitter. For permissive APIs and partner sites.",
			Tags:        []string{"fast", "low-respect"},
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
			Name:        "stealth",
			Title:       "Stealth scrape",
			Description: "Browser-friendly. Sticky proxy, longer jittered delays, escalate on bot signals.",
			Tags:        []string{"stealth", "anti-bot"},
			Policy: quarrycontracts.RunPolicy{
				Concurrency: quarrycontracts.Concurrency{PerRun: 4, PerDomain: 1},
				Delay:       quarrycontracts.Delay{MinMs: 1500, MaxMs: 5000, Jitter: true},
				Retry:       quarrycontracts.Retry{Max: 4, Backoff: quarrycontracts.BackoffExp, BaseMs: 2000},
				Proxy:       quarrycontracts.Proxy{Strategy: quarrycontracts.ProxySticky},
				Robots:      quarrycontracts.RobotsRespect,
				Ordering:    quarrycontracts.OrdFifo,
				Block: quarrycontracts.BlockPolicy{
					On:     quarrycontracts.TrigSuspectBot,
					Action: quarrycontracts.BlkEscalate,
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
}
