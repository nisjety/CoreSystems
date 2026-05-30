package driver

import (
	"context"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/go-rod/rod"
	"github.com/go-rod/rod/lib/input"
	"github.com/go-rod/rod/lib/launcher"
	"github.com/go-rod/rod/lib/proto"
	"github.com/go-rod/stealth"
	"github.com/rs/zerolog/log"
)

type RodDriver struct {
	userAgent string
	stealth   bool
	mu        sync.RWMutex
	browser   *rod.Browser
	page      *rod.Page
	last      *FetchResult
	// pool-backed fields (set only when created via NewRodDriverFromPool)
	pool      PageProvider
	releaseFn func() // non-nil when a page was acquired from the pool
	// fingerprint holds the current browser identity (nil → generate fresh)
	fingerprint *BrowserFingerprint
	// stabilizeTimeout overrides the default StabilizeWait timeout (0 → use 15 s default)
	stabilizeTimeout time.Duration
}

func NewRodDriver(userAgent string, stealthMode bool) (*RodDriver, error) {
	launchOpts := launcher.New().
		Headless(true).
		Devtools(false)

	if envEnabled("BROWSER_DEV_ALLOW_INSECURE_TLS") {
		launchOpts = launchOpts.Set("ignore-certificate-errors")
	}
	if envEnabled("BROWSER_DEV_DISABLE_WEB_SECURITY") {
		launchOpts = launchOpts.Set("disable-web-security")
	}

	if browserPath := strings.TrimSpace(os.Getenv("ROD_BROWSER_BIN")); browserPath != "" {
		launchOpts = launchOpts.Bin(browserPath)
	} else if browserPath := strings.TrimSpace(os.Getenv("ROD_CHROMIUM_BIN")); browserPath != "" {
		launchOpts = launchOpts.Bin(browserPath)
	}

	url, err := launchOpts.Launch()
	if err != nil {
		return nil, fmt.Errorf("launch rod browser: %w", err)
	}
	browser := rod.New().ControlURL(url)
	if err := browser.Connect(); err != nil {
		return nil, fmt.Errorf("failed to connect rod browser: %w", err)
	}
	return &RodDriver{userAgent: userAgent, stealth: stealthMode, browser: browser}, nil
}

// NewRodDriverFromPool creates a RodDriver that acquires pages from a shared
// pool instead of launching its own Chromium process. This eliminates the
// ~5 s cold-start cost for every JS-rendered request.
//
// pool must not be nil. The pool is responsible for page lifecycle (creation,
// stealth injection, and cleanup/reuse on Close).
func NewRodDriverFromPool(pool PageProvider) *RodDriver {
	return &RodDriver{pool: pool}
}

// WithFingerprint sets the browser fingerprint for realistic identity.
func (d *RodDriver) WithFingerprint(fp *BrowserFingerprint) *RodDriver {
	d.fingerprint = fp
	return d
}

// WithStabilizeTimeout overrides the default 15 s StabilizeWait timeout.
func (d *RodDriver) WithStabilizeTimeout(t time.Duration) *RodDriver {
	d.stabilizeTimeout = t
	return d
}

func (d *RodDriver) Name() string { return "rod" }

func (d *RodDriver) Fetch(ctx context.Context, targetURL string, opts *FetchOptions) (*FetchResult, error) {
	d.mu.Lock()
	defer d.mu.Unlock()

	var page *rod.Page
	var err error

	if d.pool != nil {
		// ── Pool-backed path ──────────────────────────────────────────────────
		// Release any previously held page back to the pool before acquiring a
		// new one (handles the case where Fetch is called multiple times on the
		// same driver instance, though that's rare with the waterfall pattern).
		if d.releaseFn != nil {
			d.releaseFn()
			d.releaseFn = nil
			d.page = nil
		}
		var releaseFn func()
		page, releaseFn, err = d.pool.GetPage(ctx)
		if err != nil {
			return nil, fmt.Errorf("acquire page from pool: %w", err)
		}
		d.releaseFn = releaseFn
	} else {
		// ── Standalone path (own Chromium process) ────────────────────────────
		if d.browser == nil {
			return nil, fmt.Errorf("rod browser is not initialized")
		}
		if d.page != nil {
			_ = d.page.Close()
			d.page = nil
		}
		page, err = d.browser.Page(proto.TargetCreateTarget{})
		if err != nil {
			return nil, fmt.Errorf("failed to create page: %w", err)
		}
		if d.stealth {
			// Use enhanced stealth with fingerprint if available, fall back to basic stealth.JS.
			var stealthScript string
			if d.fingerprint != nil {
				stealthScript = stealth.JS + "\n" + EnhancedStealthJS(d.fingerprint)
			} else {
				stealthScript = stealth.JS + "\n" + EnhancedStealthJS(nil)
			}
			if _, err := page.EvalOnNewDocument(stealthScript); err != nil {
				_ = page.Close()
				return nil, fmt.Errorf("failed to inject stealth scripts: %w", err)
			}
		}
	}

	// --- Mobile emulation ---
	if opts != nil && opts.Mobile {
		w, h, dpr := 390, 844, 3.0
		if opts.Viewport != nil {
			if opts.Viewport.Width > 0 {
				w = opts.Viewport.Width
			}
			if opts.Viewport.Height > 0 {
				h = opts.Viewport.Height
			}
			if opts.Viewport.DeviceScaleFactor > 0 {
				dpr = opts.Viewport.DeviceScaleFactor
			}
		}
		_ = proto.EmulationSetDeviceMetricsOverride{
			Width: w, Height: h, DeviceScaleFactor: dpr, Mobile: true,
		}.Call(page)
		_ = page.SetUserAgent(&proto.NetworkSetUserAgentOverride{
			UserAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1",
		})
	} else {
		if opts != nil && opts.Viewport != nil {
			_ = proto.EmulationSetDeviceMetricsOverride{
				Width: opts.Viewport.Width, Height: opts.Viewport.Height,
				DeviceScaleFactor: opts.Viewport.DeviceScaleFactor, Mobile: false,
			}.Call(page)
		}
		if d.userAgent != "" {
			_ = page.SetUserAgent(&proto.NetworkSetUserAgentOverride{UserAgent: d.userAgent})
		}
	}

	// --- Geolocation spoofing ---
	if opts != nil && opts.Location != nil {
		acc := opts.Location.Accuracy
		if acc <= 0 {
			acc = 50
		}
		lat := opts.Location.Latitude
		lon := opts.Location.Longitude
		_ = proto.EmulationSetGeolocationOverride{
			Latitude:  &lat,
			Longitude: &lon,
			Accuracy:  &acc,
		}.Call(page)
	}

	// --- Ad / tracker blocking ---
	// Use page.HijackRequests() (not d.browser.HijackRequests()) so interception
	// is scoped to this page only — safe when a shared browser pool is in use.
	var adRouter *rod.HijackRouter
	if opts != nil && opts.BlockAds {
		adRouter = page.HijackRequests()
		for _, pattern := range adBlockPatterns {
			p := pattern
			if err := adRouter.Add(p, "", func(ctx *rod.Hijack) {
				ctx.Response.Fail(proto.NetworkErrorReasonBlockedByClient)
			}); err != nil {
				_ = page.Close()
				return nil, fmt.Errorf("configure ad blocking for %q: %w", p, err)
			}
		}
		go adRouter.Run()
	}

	// --- Media blocking (images, fonts, media) ---
	// Saves 300-800 ms on media-heavy pages; skip when taking screenshots.
	// Use page.HijackRequests() — page-scoped, safe for concurrent pooled pages.
	var mediaRouter *rod.HijackRouter
	if opts != nil && opts.BlockMedia {
		mediaRouter = page.HijackRequests()
		mediaTypes := []string{"*.jpg", "*.jpeg", "*.png", "*.gif", "*.webp", "*.svg",
			"*.ico", "*.woff", "*.woff2", "*.ttf", "*.eot", "*.mp4", "*.mp3", "*.webm"}
		for _, pattern := range mediaTypes {
			p := pattern
			if err := mediaRouter.Add(p, "", func(ctx *rod.Hijack) {
				ctx.Response.Fail(proto.NetworkErrorReasonBlockedByClient)
			}); err != nil {
				if adRouter != nil {
					_ = adRouter.Stop()
				}
				_ = page.Close()
				return nil, fmt.Errorf("configure media blocking for %q: %w", p, err)
			}
		}
		go mediaRouter.Run()
	}

	// Register wait-for-navigation listener BEFORE Navigate() so we don't miss the event.
	waitNav := page.WaitNavigation(proto.PageLifecycleEventNameNetworkAlmostIdle)

	if err := page.Navigate(targetURL); err != nil {
		if adRouter != nil {
			_ = adRouter.Stop()
		}
		if mediaRouter != nil {
			_ = mediaRouter.Stop()
		}
		_ = page.Close()
		return nil, fmt.Errorf("navigate failed: %w", err)
	}

	// Wait until network activity has almost stopped (catches CSR/SPA content).
	waitNav()

	// ── Page stabilisation ────────────────────────────────────────────────
	// StabilizeWait uses Rod's native WaitDOMStable → WaitStable →
	// WaitRequestIdle chain. Framework-agnostic: no SPA fingerprinting,
	// no framework-specific JS. Works reliably for SSR, CSR, and hybrid
	// pages without knowing which framework rendered the page.
	stabilizeTimeout := d.stabilizeTimeout
	if stabilizeTimeout <= 0 {
		stabilizeTimeout = 15 * time.Second
	}
	log.Debug().Str("url", targetURL).Msg("rod: starting stabilize wait")
	StabilizeWait(ctx, page, stabilizeTimeout)

	if opts != nil && opts.WaitFor > 0 {
		if err := d.waitLocked(ctx, opts.WaitFor); err != nil {
			if adRouter != nil {
				_ = adRouter.Stop()
			}
			if mediaRouter != nil {
				_ = mediaRouter.Stop()
			}
			_ = page.Close()
			return nil, err
		}
	}

	// --- Auto-scroll to trigger lazy-loaded content ---
	if opts != nil && opts.AutoScroll {
		SmartAutoScroll(ctx, page, 20, 200)
	}

	html, err := page.HTML()
	if err != nil {
		if adRouter != nil {
			_ = adRouter.Stop()
		}
		if mediaRouter != nil {
			_ = mediaRouter.Stop()
		}
		_ = page.Close()
		return nil, fmt.Errorf("get html failed: %w", err)
	}

	// Stop hijack routers before handing page back (page stays open for actions).
	if adRouter != nil {
		_ = adRouter.Stop()
		adRouter = nil
	}
	if mediaRouter != nil {
		_ = mediaRouter.Stop()
		mediaRouter = nil
	}

	links := extractLinksFromHTML(html)
	result := &FetchResult{
		URL:         targetURL,
		Status:      200,
		ContentType: "text/html",
		HTML:        html,
		RawHTML:     html,
		Links:       links,
		Rendered:    true,
	}

	d.page = page
	d.last = result
	return result, nil
}

func (d *RodDriver) HTML(ctx context.Context) (string, error) {
	select {
	case <-ctx.Done():
		return "", ctx.Err()
	default:
	}
	d.mu.RLock()
	defer d.mu.RUnlock()
	if d.page != nil {
		html, err := d.page.HTML()
		if err == nil {
			return html, nil
		}
	}
	if d.last != nil {
		return d.last.HTML, nil
	}
	return "", nil
}

func (d *RodDriver) Click(ctx context.Context, selector string) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return err
	}
	if d.page == nil {
		return fmt.Errorf("page is not initialized")
	}
	el, err := d.page.Element(selector)
	if err != nil {
		return fmt.Errorf("find element failed: %w", err)
	}
	if err := el.Click(proto.InputMouseButtonLeft, 1); err != nil {
		return fmt.Errorf("click failed: %w", err)
	}
	return nil
}

func (d *RodDriver) Type(ctx context.Context, selector, text string) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return err
	}
	if d.page == nil {
		return fmt.Errorf("page is not initialized")
	}
	el, err := d.page.Element(selector)
	if err != nil {
		return fmt.Errorf("find element failed: %w", err)
	}
	if err := el.Input(text); err != nil {
		return fmt.Errorf("type failed: %w", err)
	}
	return nil
}

func (d *RodDriver) Press(ctx context.Context, key string) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return err
	}
	if d.page == nil {
		return fmt.Errorf("page is not initialized")
	}
	code, ok := mapInputKey(key)
	if !ok {
		return fmt.Errorf("unsupported key: %s", key)
	}
	if err := d.page.Keyboard.Press(code); err != nil {
		return fmt.Errorf("press key failed: %w", err)
	}
	return nil
}

func envEnabled(key string) bool {
	value := strings.TrimSpace(os.Getenv(key))
	return value == "1" || strings.EqualFold(value, "true")
}

func (d *RodDriver) Wait(ctx context.Context, milliseconds int) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.waitLocked(ctx, milliseconds)
}

func (d *RodDriver) waitLocked(ctx context.Context, milliseconds int) error {
	if milliseconds <= 0 {
		return nil
	}
	t := time.NewTimer(time.Duration(milliseconds) * time.Millisecond)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-t.C:
		return nil
	}
}

func (d *RodDriver) Scroll(ctx context.Context, direction string) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return err
	}
	if d.page == nil {
		return fmt.Errorf("page is not initialized")
	}
	dir := strings.ToLower(strings.TrimSpace(direction))
	var script string
	switch dir {
	case "up":
		script = `window.scrollBy(0, -600);`
	case "left":
		script = `window.scrollBy(-600, 0);`
	case "right":
		script = `window.scrollBy(600, 0);`
	case "top":
		script = `window.scrollTo(0, 0);`
	case "bottom":
		script = `window.scrollTo(0, document.body.scrollHeight);`
	default:
		script = `window.scrollBy(0, 600);`
	}
	if _, err := d.page.Eval(script); err != nil {
		return fmt.Errorf("scroll failed: %w", err)
	}
	return nil
}

func (d *RodDriver) Screenshot(ctx context.Context, fullPage bool) ([]byte, error) {
	d.mu.Lock()
	defer d.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if d.page == nil {
		return nil, fmt.Errorf("page is not initialized")
	}
	return d.page.Screenshot(fullPage, nil)
}

// adBlockPatterns is the list of glob patterns matched against request URLs to block ads.
var adBlockPatterns = []string{
	"*doubleclick.net*",
	"*googlesyndication.com*",
	"*google-analytics.com*",
	"*googletagmanager.com*",
	"*facebook.com/tr*",
	"*amazon-adsystem.com*",
	"*adsrvr.org*",
	"*quantserve.com*",
	"*scorecardresearch.com*",
	"*moatads.com*",
	"*pubmatic.com*",
}

func (d *RodDriver) EvalJS(ctx context.Context, script string) (interface{}, error) {
	d.mu.Lock()
	defer d.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if d.page == nil {
		return nil, fmt.Errorf("page is not initialized")
	}
	// Rod's Eval expects a JavaScript function definition or expression.
	// We wrap the script in a function that returns the value.
	wrappedScript := fmt.Sprintf("() => { return %s }", script)
	obj, err := d.page.Eval(wrappedScript)
	if err != nil {
		return nil, fmt.Errorf("eval failed: %w", err)
	}
	var val interface{}
	if err := obj.Value.Unmarshal(&val); err != nil {
		// Return raw JSON string on unmarshal failure.
		return obj.Value.Str(), nil
	}
	return val, nil
}

func (d *RodDriver) GeneratePDF(ctx context.Context) ([]byte, error) {
	d.mu.Lock()
	defer d.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if d.page == nil {
		return nil, fmt.Errorf("page is not initialized")
	}
	reader, err := d.page.PDF(&proto.PagePrintToPDF{
		PrintBackground: true,
	})
	if err != nil {
		return nil, fmt.Errorf("pdf generation failed: %w", err)
	}
	return io.ReadAll(reader)
}

func (d *RodDriver) Close() error {
	d.mu.Lock()
	defer d.mu.Unlock()

	// Pool path: return the page to the pool via the release function.
	if d.releaseFn != nil {
		d.releaseFn()
		d.releaseFn = nil
		d.page = nil
		return nil
	}

	// Standalone path: close our own page and browser.
	if d.page != nil {
		_ = d.page.Close()
		d.page = nil
	}
	if d.browser != nil {
		if err := d.browser.Close(); err != nil {
			return err
		}
		d.browser = nil
	}
	return nil
}

func extractLinksFromHTML(html string) []string {
	links := make([]string, 0)
	parts := strings.Split(html, "href=")
	if len(parts) < 2 {
		return links
	}
	for _, part := range parts[1:] {
		if len(part) < 2 {
			continue
		}
		quote := part[0]
		if quote != '\'' && quote != '"' {
			continue
		}
		end := strings.IndexByte(part[1:], quote)
		if end < 0 {
			continue
		}
		href := strings.TrimSpace(part[1 : 1+end])
		if href == "" {
			continue
		}
		links = append(links, href)
	}
	return links
}

func mapInputKey(key string) (input.Key, bool) {
	switch strings.ToLower(strings.TrimSpace(key)) {
	case "enter":
		return input.Enter, true
	case "escape", "esc":
		return input.Escape, true
	case "tab":
		return input.Tab, true
	case "space":
		return input.Space, true
	case "backspace":
		return input.Backspace, true
	case "arrowup", "up":
		return input.ArrowUp, true
	case "arrowdown", "down":
		return input.ArrowDown, true
	case "arrowleft", "left":
		return input.ArrowLeft, true
	case "arrowright", "right":
		return input.ArrowRight, true
	default:
		return 0, false
	}
}
