package driver

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/driver/history"
)

// Waterfall selects and executes a PageDriver from a priority-ordered list of
// engines. On Fetch failure it automatically falls back to the next capable
// engine, giving every URL the highest chance of success without caller
// involvement.
//
// After a successful Fetch, all subsequent calls (Click, Type, HTML, etc.) are
// forwarded to the engine that succeeded, so the caller sees a single
// PageDriver.
type Waterfall struct {
	engines []Engine
	active  PageDriver // the engine that won the Fetch race
	advisor *history.Advisor
}

// NewWaterfall creates a waterfall from the supplied engines, sorted by
// ascending priority (lower = tried first).
func NewWaterfall(engines []Engine) *Waterfall {
	sorted := make([]Engine, len(engines))
	copy(sorted, engines)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].Priority < sorted[j].Priority })
	return &Waterfall{engines: sorted}
}

// DefaultWaterfall returns the standard four-engine waterfall used by Quarry.
// PDF (documents) → Colly (fast HTTP) → TLS-fetch (browser fingerprint) → Rod (headless Chrome).
//
// pool is optional. When non-nil the Rod engine draws pages from the shared
// pool (no Chromium cold-start). When nil the Rod engine launches its own
// Chromium process per request (legacy behaviour, safe for tests).
//
// aiCoreBaseURL is the Model Plane v2 base URL used for OCR-backed PDF parsing.
// Pass an empty string to disable OCR.
func DefaultWaterfall(userAgent string, stealth bool, pool PageProvider, aiCoreBaseURL string) *Waterfall {
	var rodFactory func() (PageDriver, error)
	if pool != nil {
		rodFactory = func() (PageDriver, error) { return NewRodDriverFromPool(pool), nil }
	} else {
		rodFactory = func() (PageDriver, error) { return NewRodDriver(userAgent, stealth) }
	}

	return NewWaterfall([]Engine{
		{
			Name:         "pdf",
			Priority:     50,
			Capabilities: CapHTTP | CapDocument,
			Factory:      func() (PageDriver, error) { return NewPDFEngine(userAgent, "auto", aiCoreBaseURL), nil },
		},
		{
			Name:         "colly",
			Priority:     100,
			Capabilities: CapHTTP,
			Factory:      func() (PageDriver, error) { return NewCollyDriver(userAgent), nil },
		},
		{
			Name:         "tls-fetch",
			Priority:     150,
			Capabilities: CapHTTP | CapStealth,
			Factory:      func() (PageDriver, error) { return NewTLSFetchDriver(userAgent), nil },
		},
		{
			Name:     "rod",
			Priority: 200,
			Capabilities: CapHTTP | CapJavaScript | CapScreenshot | CapPDF |
				CapActions | CapMobile | CapGeo | CapAdBlock | CapStealth,
			Factory: rodFactory,
		},
	})
}

// requiredCapabilities maps SelectionInput fields to the capabilities they
// demand.
func requiredCapabilities(input SelectionInput) Capability {
	var caps Capability
	caps |= CapHTTP // every request needs at least basic HTTP

	// Auto-detect document URLs if not explicitly set.
	if input.IsDocument || (input.TargetURL != "" && isDocumentURL(input.TargetURL)) {
		caps |= CapDocument
		return caps // document engines handle everything
	}

	if input.NeedJavaScript {
		caps |= CapJavaScript
	}
	if input.NeedScreenshot {
		caps |= CapScreenshot
	}
	if input.HasActions {
		caps |= CapActions
	}
	if input.WaitForMs > 0 {
		caps |= CapJavaScript // waitFor requires a real browser
	}
	if input.Mobile {
		caps |= CapMobile
	}
	if input.HasGeo {
		caps |= CapGeo
	}
	if input.BlockAds {
		caps |= CapAdBlock
	}

	for _, f := range input.Formats {
		switch strings.ToLower(strings.TrimSpace(f)) {
		case "screenshot":
			caps |= CapScreenshot
		case "pdf":
			caps |= CapPDF
		}
	}

	return caps
}

// SetAdvisor attaches an Advisor that reorders engine candidates based on
// per-domain historical performance. Nil-safe: if no advisor is set, the
// default priority order is used.
func (w *Waterfall) SetAdvisor(a *history.Advisor) *Waterfall {
	w.advisor = a
	return w
}

// Select returns a WaterfallDriver that, on Fetch, will try each capable
// engine in priority order. If a hard requirement (screenshot, actions, etc.)
// eliminates all lightweight engines, those are skipped automatically.
// When an Advisor is attached, engine order is adjusted by domain history.
func (w *Waterfall) Select(input SelectionInput) (PageDriver, error) {
	required := requiredCapabilities(input)

	// Filter to engines that satisfy all hard requirements.
	var candidates []Engine
	for _, e := range w.engines {
		if e.Supports(required) {
			candidates = append(candidates, e)
		}
	}
	if len(candidates) == 0 {
		return nil, fmt.Errorf("no engine supports required capabilities %d", required)
	}

	// Reorder candidates according to domain-specific performance history.
	if w.advisor != nil && input.TargetURL != "" {
		domain := history.ExtractDomain(input.TargetURL)
		advCtx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
		defer cancel()
		rec := w.advisor.Recommend(advCtx, domain)
		candidates = reorderByAdvisor(candidates, rec.Engines)
	}

	var store *history.Store
	if w.advisor != nil {
		store = w.advisor.Store()
	}

	return &WaterfallDriver{
		candidates: candidates,
		required:   required,
		store:      store,
	}, nil
}

// reorderByAdvisor reorders candidates so that engines listed in preferred
// come first (in the order given), with any remaining candidates appended at
// the end in their original relative order.
func reorderByAdvisor(candidates []Engine, preferred []string) []Engine {
	if len(preferred) == 0 {
		return candidates
	}
	rank := make(map[string]int, len(preferred))
	for i, name := range preferred {
		rank[name] = i
	}
	result := make([]Engine, 0, len(candidates))
	rest := make([]Engine, 0)
	for _, c := range candidates {
		if _, ok := rank[c.Name]; ok {
			result = append(result, c)
		} else {
			rest = append(rest, c)
		}
	}
	sort.Slice(result, func(i, j int) bool {
		return rank[result[i].Name] < rank[result[j].Name]
	})
	return append(result, rest...)
}

// Fetch tries each candidate engine in order. The first successful Fetch wins;
// failures cause the engine to be closed and the next one tried.
func (w *Waterfall) Fetch(ctx context.Context, targetURL string, opts *FetchOptions, input SelectionInput) (*FetchResult, PageDriver, error) {
	required := requiredCapabilities(input)

	var lastErr error
	for _, eng := range w.engines {
		if !eng.Supports(required) {
			continue
		}

		drv, err := eng.Factory()
		if err != nil {
			log.Warn().Err(err).Str("engine", eng.Name).Msg("engine init failed, trying next")
			lastErr = err
			continue
		}

		result, err := drv.Fetch(ctx, targetURL, opts)
		if err != nil {
			log.Warn().Err(err).Str("engine", eng.Name).Str("url", targetURL).Msg("engine fetch failed, trying next")
			_ = drv.Close()
			lastErr = err
			continue
		}

		log.Info().Str("engine", eng.Name).Str("url", targetURL).Int("status", result.Status).Msg("engine fetch succeeded")
		return result, drv, nil
	}

	return nil, nil, fmt.Errorf("all engines failed for %s: %w", targetURL, lastErr)
}

// ---------- WaterfallDriver: transparent PageDriver wrapper ----------

// WaterfallDriver implements PageDriver by lazily trying engines on Fetch
// and forwarding all subsequent calls to the engine that succeeded.
type WaterfallDriver struct {
	candidates []Engine
	required   Capability
	active     PageDriver
	targetURL  string         // set on first Fetch for outcome recording
	store      *history.Store // optional; nil = no recording
}

func (wd *WaterfallDriver) Name() string {
	if wd.active != nil {
		return "waterfall:" + wd.active.Name()
	}
	return "waterfall"
}

func (wd *WaterfallDriver) Fetch(ctx context.Context, targetURL string, opts *FetchOptions) (*FetchResult, error) {
	wd.targetURL = targetURL
	domain := history.ExtractDomain(targetURL)

	var lastErr error
	challengeDetected := false
	for _, eng := range wd.candidates {
		drv, err := eng.Factory()
		if err != nil {
			log.Warn().Err(err).Str("engine", eng.Name).Msg("engine init failed, trying next")
			lastErr = err
			continue
		}

		start := time.Now()
		result, err := drv.Fetch(ctx, targetURL, opts)
		latencyMs := int(time.Since(start).Milliseconds())
		if err != nil {
			log.Warn().Err(err).Str("engine", eng.Name).Str("url", targetURL).Msg("engine fetch failed, trying next")
			_ = drv.Close()
			lastErr = err
			if wd.store != nil {
				go wd.store.Record(context.Background(), domain, eng.Name, history.OutcomeRecord{ //nolint:contextcheck
					Success: false, LatencyMs: latencyMs,
				})
			}
			continue
		}

		// Challenge detection: if a lightweight engine hits a bot challenge,
		// close it and escalate to the next engine (likely Rod with stealth).
		if result != nil && IsChallenge(result.Status, result.HTML) {
			challenge := DetectChallenge(result.Status, result.HTML)
			log.Warn().
				Str("engine", eng.Name).
				Str("url", targetURL).
				Str("challenge", challenge.String()).
				Int("status", result.Status).
				Msg("bot challenge detected, escalating to next engine")
			_ = drv.Close()
			challengeDetected = true
			if wd.store != nil {
				go wd.store.Record(context.Background(), domain, eng.Name, history.OutcomeRecord{ //nolint:contextcheck
					Success: false, LatencyMs: latencyMs,
				})
			}
			continue
		}

		log.Info().Str("engine", eng.Name).Str("url", targetURL).Int("status", result.Status).Msg("waterfall: engine selected")
		if wd.store != nil {
			go wd.store.Record(context.Background(), domain, eng.Name, history.OutcomeRecord{ //nolint:contextcheck
				Success: true, LatencyMs: latencyMs,
			})
		}
		// Close any previously active driver (shouldn't happen in normal flow).
		if wd.active != nil {
			_ = wd.active.Close()
		}
		wd.active = drv
		return result, nil
	}
	if challengeDetected {
		return nil, fmt.Errorf("all engines blocked by bot challenge for %s: %w", targetURL, lastErr)
	}
	return nil, fmt.Errorf("all engines failed for %s: %w", targetURL, lastErr)
}

func (wd *WaterfallDriver) HTML(ctx context.Context) (string, error) {
	if wd.active == nil {
		return "", fmt.Errorf("no active engine; call Fetch first")
	}
	return wd.active.HTML(ctx)
}

func (wd *WaterfallDriver) Click(ctx context.Context, selector string) error {
	if wd.active == nil {
		return fmt.Errorf("no active engine")
	}
	return wd.active.Click(ctx, selector)
}

func (wd *WaterfallDriver) Type(ctx context.Context, selector, text string) error {
	if wd.active == nil {
		return fmt.Errorf("no active engine")
	}
	return wd.active.Type(ctx, selector, text)
}

func (wd *WaterfallDriver) Press(ctx context.Context, key string) error {
	if wd.active == nil {
		return fmt.Errorf("no active engine")
	}
	return wd.active.Press(ctx, key)
}

func (wd *WaterfallDriver) Wait(ctx context.Context, milliseconds int) error {
	if wd.active == nil {
		return fmt.Errorf("no active engine")
	}
	return wd.active.Wait(ctx, milliseconds)
}

func (wd *WaterfallDriver) Scroll(ctx context.Context, direction string) error {
	if wd.active == nil {
		return fmt.Errorf("no active engine")
	}
	return wd.active.Scroll(ctx, direction)
}

func (wd *WaterfallDriver) Screenshot(ctx context.Context, fullPage bool) ([]byte, error) {
	if wd.active == nil {
		return nil, fmt.Errorf("no active engine")
	}
	return wd.active.Screenshot(ctx, fullPage)
}

func (wd *WaterfallDriver) EvalJS(ctx context.Context, script string) (interface{}, error) {
	if wd.active == nil {
		return nil, fmt.Errorf("no active engine")
	}
	return wd.active.EvalJS(ctx, script)
}

func (wd *WaterfallDriver) GeneratePDF(ctx context.Context) ([]byte, error) {
	if wd.active == nil {
		return nil, fmt.Errorf("no active engine")
	}
	return wd.active.GeneratePDF(ctx)
}

func (wd *WaterfallDriver) Close() error {
	if wd.active != nil {
		return wd.active.Close()
	}
	return nil
}
