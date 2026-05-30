package scraper

import (
	"context"
	"fmt"
	"os"
	"sync"
	"time"

	"github.com/go-rod/rod"
	"github.com/go-rod/rod/lib/launcher"
	"github.com/go-rod/rod/lib/proto"
	"github.com/go-rod/stealth"
	"github.com/triodelab/quarry/internal/driver"
)

// pooledPage pairs a tab with its own incognito browser context.
// Using an incognito context per slot guarantees cookie / storage isolation
// between different tenants even when the same pool slot is reused.
type pooledPage struct {
	page      *rod.Page
	incognito *rod.Browser
}

// BrowserPool manages a pool of pre-warmed, incognito-isolated browser pages
// backed by a single shared Chromium process.
//
// N+1 invariant: whenever the idle queue drains to zero (a slot is claimed),
// a background goroutine immediately starts warming a replacement so there is
// always a ready page waiting for the next request.
type BrowserPool struct {
	browser  *rod.Browser
	launcher *launcher.Launcher
	idle     chan *pooledPage // pre-warmed pages ready to hand out
	active   chan struct{}    // concurrency semaphore
	mu       sync.Mutex       // guards newPooledPage
}

// NewBrowserPool creates a new browser pool backed by a single Chromium
// process. size is the maximum number of concurrent pages; the pool
// pre-warms one idle page at startup.
func NewBrowserPool(size int, headless bool) (*BrowserPool, error) {
	if size <= 0 {
		size = 1
	}

	l := launcher.New().
		Headless(headless).
		Devtools(false).
		Set("disable-gpu").
		Set("no-sandbox").
		Set("disable-setuid-sandbox")

	// Check for system chromium in Docker/CI environments
	if browserPath := os.Getenv("ROD_BROWSER_BIN"); browserPath != "" {
		l = l.Bin(browserPath)
	} else if browserPath := os.Getenv("ROD_CHROMIUM_BIN"); browserPath != "" {
		l = l.Bin(browserPath)
	}

	controlURL, err := l.Launch()
	if err != nil {
		return nil, fmt.Errorf("launch browser: %w", err)
	}

	browser := rod.New().ControlURL(controlURL)
	if err := browser.Connect(); err != nil {
		return nil, fmt.Errorf("connect browser: %w", err)
	}

	p := &BrowserPool{
		browser:  browser,
		launcher: l,
		idle:     make(chan *pooledPage, size+1),
		active:   make(chan struct{}, size),
	}

	// Pre-warm one idle page so the first request doesn't pay cold-start cost.
	if pp, warmErr := p.newPooledPage(); warmErr == nil {
		p.idle <- pp
	}

	return p, nil
}

// newPooledPage creates a fresh incognito context + tab with stealth scripts injected.
func (p *BrowserPool) newPooledPage() (*pooledPage, error) {
	p.mu.Lock()
	defer p.mu.Unlock()

	incog, err := p.browser.Incognito()
	if err != nil {
		return nil, fmt.Errorf("create incognito context: %w", err)
	}

	page, err := incog.Page(proto.TargetCreateTarget{URL: "about:blank"})
	if err != nil {
		_ = incog.Close()
		return nil, fmt.Errorf("create page in incognito context: %w", err)
	}

	if _, err := page.EvalOnNewDocument(stealth.JS); err != nil {
		_ = page.Close()
		_ = incog.Close()
		return nil, fmt.Errorf("inject stealth scripts: %w", err)
	}

	// Inject enhanced stealth with a per-page fingerprint for realistic identity.
	fp := driver.GenerateFingerprint()
	enhancedJS := driver.EnhancedStealthJS(&fp)
	if _, err := page.EvalOnNewDocument(enhancedJS); err != nil {
		_ = page.Close()
		_ = incog.Close()
		return nil, fmt.Errorf("inject enhanced stealth scripts: %w", err)
	}

	// Set viewport dimensions from fingerprint for consistency.
	_ = proto.EmulationSetDeviceMetricsOverride{
		Width: fp.ViewportWidth, Height: fp.ViewportHeight,
		DeviceScaleFactor: fp.DevicePixelRatio, Mobile: false,
	}.Call(page)
	_ = page.SetUserAgent(&proto.NetworkSetUserAgentOverride{
		UserAgent: fp.UserAgent,
	})

	return &pooledPage{page: page, incognito: incog}, nil
}

// GetPage acquires a page from the pool.
//
// It blocks until a concurrency slot is available (or ctx is cancelled).
// The returned release func MUST be called when the caller is done; it resets
// the page state and returns the slot to the idle queue.
func (p *BrowserPool) GetPage(ctx context.Context) (*rod.Page, func(), error) {
	// Acquire concurrency slot.
	select {
	case p.active <- struct{}{}:
	case <-ctx.Done():
		return nil, nil, ctx.Err()
	}

	// Grab a pre-warmed idle page, or create one inline when queue is empty.
	var pp *pooledPage
	select {
	case pp = <-p.idle:
		// Got a warm page — fast path.
	default:
		// No idle page available; create one now.
		var createErr error
		pp, createErr = p.newPooledPage()
		if createErr != nil {
			<-p.active // release slot
			return nil, nil, createErr
		}
	}

	// N+1 invariant: if idle queue is now empty, start warming a replacement
	// in the background so the next caller doesn't have to wait for cold-start.
	if len(p.idle) == 0 {
		go func() {
			newPP, err := p.newPooledPage()
			if err != nil {
				return
			}
			select {
			case p.idle <- newPP:
				// stored in idle queue
			default:
				// idle queue full (pool was returned before warm finished)
				_ = newPP.page.Close()
				_ = newPP.incognito.Close()
			}
		}()
	}

	release := func() {
		defer func() { <-p.active }() // always release concurrency slot

		// Reset page state so the next request gets a clean slate.
		resetCtx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()

		resetErr := pp.page.Context(resetCtx).Navigate("about:blank")
		if resetErr != nil {
			// Page is broken — discard and let N+1 warmer cover the gap.
			_ = pp.page.Close()
			_ = pp.incognito.Close()
			return
		}

		// Clear cookies for the incognito context (privacy between tenant reuses).
		_ = proto.NetworkClearBrowserCookies{}.Call(pp.incognito)

		// Return to idle queue.
		select {
		case p.idle <- pp:
			// returned to pool
		default:
			// queue unexpectedly full — discard cleanly
			_ = pp.page.Close()
			_ = pp.incognito.Close()
		}
	}

	return pp.page, release, nil
}

// Close closes all idle pages and the shared browser process.
func (p *BrowserPool) Close() error {
	if p == nil {
		return nil
	}

	// Drain and close all idle pages.
	for {
		select {
		case pp := <-p.idle:
			_ = pp.page.Close()
			_ = pp.incognito.Close()
		default:
			goto drained
		}
	}
drained:

	if p.browser != nil {
		if err := p.browser.Close(); err != nil {
			return err
		}
	}
	if p.launcher != nil {
		p.launcher.Cleanup()
	}
	return nil
}
