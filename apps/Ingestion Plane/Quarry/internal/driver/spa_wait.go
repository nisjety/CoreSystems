package driver

import (
	"context"
	"time"

	"github.com/go-rod/rod"
	"github.com/go-rod/rod/lib/proto"
	"github.com/rs/zerolog/log"
)

// noiseExcludes are URL substrings for requests that should NOT be waited on
// when calling WaitRequestIdle. Analytics, tracking, and persistent connections
// never go truly idle but are irrelevant to whether page content is ready.
var noiseExcludes = []string{
	"analytics", "gtm", "doubleclick", "segment", "hotjar",
	"intercom", "crisp", "drift", "heap", "mixpanel", "amplitude",
	"sentry", "datadog", "newrelic", "pingdom",
}

// noiseResourceTypes are Chrome resource types excluded from WaitRequestIdle.
// WebSocket, EventSource, and media streams stay open indefinitely.
var noiseResourceTypes = []proto.NetworkResourceType{
	proto.NetworkResourceTypeWebSocket,
	proto.NetworkResourceTypeEventSource,
	proto.NetworkResourceTypeMedia,
}

// StabilizeWait waits for a rendered page to fully settle using Rod's native
// stability primitives. It is framework-agnostic: no SPA detection, no
// framework-specific JS. The three stages run sequentially:
//
//  1. WaitDOMStable — waits until DOM-tree mutation rate drops below 1% for 1.5 s
//  2. WaitStable — waits until the page-level stability metric is quiet for 1 s
//  3. WaitRequestIdle — waits until no non-noise network requests have been
//     active for 500 ms (analytics/tracking/websockets excluded)
//
// Each stage has its own sub-timeout so a misbehaving page cannot stall the
// overall render pipeline. The entire wait is capped at timeout.
func StabilizeWait(ctx context.Context, page *rod.Page, timeout time.Duration) {
	if timeout <= 0 {
		timeout = 15 * time.Second
	}
	outer, outerCancel := context.WithTimeout(ctx, timeout)
	defer outerCancel()

	// Stage 1: DOM churn — wait for structural mutations to settle.
	domCtx, domCancel := context.WithTimeout(outer, 5*time.Second)
	if err := page.Context(domCtx).WaitDOMStable(1500*time.Millisecond, 0.01); err != nil {
		log.Debug().Err(err).Msg("spa_wait: WaitDOMStable timed out (non-fatal)")
	}
	domCancel()

	if outer.Err() != nil {
		return
	}

	// Stage 2: Page-level stability (covers layout, style, script execution).
	stableCtx, stableCancel := context.WithTimeout(outer, 3*time.Second)
	if err := page.Context(stableCtx).WaitStable(1 * time.Second); err != nil {
		log.Debug().Err(err).Msg("spa_wait: WaitStable timed out (non-fatal)")
	}
	stableCancel()

	if outer.Err() != nil {
		return
	}

	// Stage 3: Network idle — no meaningful requests for 500 ms.
	// Exclude analytics and persistent connections that never go idle.
	reqCtx, reqCancel := context.WithTimeout(outer, 5*time.Second)
	waitFn := page.Context(reqCtx).WaitRequestIdle(
		500*time.Millisecond,
		nil,              // watch all request URLs
		noiseExcludes,    // except analytics/tracking
		noiseResourceTypes, // and persistent connection types
	)
	waitFn()
	reqCancel()

	log.Debug().Msg("spa_wait: page stable (DOM + stability + network idle)")
}

// SmartAutoScroll scrolls the page incrementally with MutationObserver-based
// quiet detection between scrolls, and stops early when no new content loads.
func SmartAutoScroll(ctx context.Context, page *rod.Page, maxScrolls int, quietMs int) {
	if maxScrolls <= 0 {
		maxScrolls = 20
	}
	if quietMs <= 0 {
		quietMs = 200
	}

	// First, trigger lazy-loaded images by injecting an IntersectionObserver
	// that sets data-src → src for common lazy-load patterns.
	_, _ = page.Context(ctx).Eval(`() => {
		const imgs = document.querySelectorAll('img[data-src], img[loading="lazy"]');
		if (imgs.length > 0) {
			const obs = new IntersectionObserver((entries) => {
				entries.forEach(e => {
					if (e.isIntersecting) {
						const img = e.target;
						if (img.dataset.src) { img.src = img.dataset.src; }
						obs.unobserve(img);
					}
				});
			});
			imgs.forEach(img => obs.observe(img));
		}
	}`)

	prevHeight := 0
	for i := 0; i < maxScrolls; i++ {
		if ctx.Err() != nil {
			return
		}

		obj, err := page.Context(ctx).Eval(`() => document.body.scrollHeight`)
		if err != nil {
			return
		}
		height := obj.Value.Int()
		if height > 0 && height == prevHeight {
			return // no new content loaded
		}
		prevHeight = height

		// Scroll to bottom.
		_, _ = page.Context(ctx).Eval(`() => window.scrollTo(0, document.body.scrollHeight)`)

		// Wait for DOM to settle after each scroll using Rod's native WaitDOMStable.
		quietCtx, quietCancel := context.WithTimeout(ctx, 3*time.Second)
		_ = page.Context(quietCtx).WaitDOMStable(time.Duration(quietMs)*time.Millisecond, 0.01)
		quietCancel()
	}
}
