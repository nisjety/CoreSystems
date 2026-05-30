package driver

// spa_strategies.go provides framework-specific JavaScript wait snippets
// that detect hydration completion for popular SPA frameworks.
//
// Each function returns a self-executing JS expression suitable for
// page.Eval() that resolves a Promise when the framework has finished
// hydrating the server-rendered (or client-rendered) HTML.

// FrameworkWaitJS returns a JS snippet for the given framework name.
// Returns empty string if the framework is unknown or needs no special wait.
func FrameworkWaitJS(framework string) string {
	switch framework {
	case "react", "nextjs":
		return reactNextWaitJS
	case "vue", "nuxt":
		return vueNuxtWaitJS
	case "angular":
		return angularWaitJS
	case "svelte", "sveltekit":
		return svelteWaitJS
	case "gatsby":
		return gatsbyWaitJS
	case "remix":
		return remixWaitJS
	case "astro":
		return astroWaitJS
	case "qwik":
		return qwikWaitJS
	default:
		return ""
	}
}

// reactNextWaitJS waits for React/Next.js content to actually render.
//
// Previous logic resolved immediately when __NEXT_DATA__ was present — but that
// script is injected by SSR before any client-side hydration, so it fires on the
// loading-placeholder state ("Laster inn...") rather than the real page content.
//
// New strategy (three-tier):
//  1. Semantic main content: wait for <main>/<article>/role=main to contain
//     ≥300 chars. Cookie/consent overlays sit in dialogs and are not counted.
//  2. Root container fallback: #__next / #root total text ≥600 chars after
//     subtracting known consent-overlay text, so a cookie banner alone won't
//     false-positive.
//  3. Hard timeout: resolve unconditionally after 12 s so the crawl isn't
//     blocked indefinitely on pages that genuinely have sparse content.
const reactNextWaitJS = `() => new Promise(resolve => {
	const MAIN_MIN = 300;
	const ROOT_MIN = 600;

	const ready = () => {
		// Tier 1 — semantic content containers (skip overlays/dialogs).
		const main = document.querySelector('main')
		          || document.querySelector('[role="main"]')
		          || document.querySelector('article');
		if (main && (main.innerText || '').trim().length >= MAIN_MIN) return true;

		// Tier 2 — root container minus consent/cookie overlay text.
		const root = document.getElementById('__next')
		          || document.getElementById('root')
		          || document.querySelector('[data-reactroot]');
		const base = root || document.body;
		if (!base) return false;
		let len = (base.innerText || '').trim().length;
		base.querySelectorAll(
			'[id*="cookie"],[class*="cookie"],[id*="consent"],[class*="consent"],' +
			'[id*="gdpr"],[class*="gdpr"],[role="dialog"],[role="alertdialog"]'
		).forEach(function(el) { len -= (el.innerText || '').length; });
		return len >= ROOT_MIN;
	};

	if (ready()) { resolve(true); return; }
	const obs = new MutationObserver(function() {
		if (ready()) { obs.disconnect(); resolve(true); }
	});
	obs.observe(document.documentElement, { childList: true, subtree: true, characterData: true });
	setTimeout(function() { obs.disconnect(); resolve(true); }, 12000);
})`

// vueNuxtWaitJS waits for Vue/Nuxt hydration by checking for:
// 1. __NUXT__ global (Nuxt hydration state)
// 2. Multiple elements with data-v-* attributes (Vue scoped CSS = hydrated components)
const vueNuxtWaitJS = `() => new Promise(resolve => {
	const check = () => {
		if (window.__NUXT__) { resolve(true); return; }
		const vueEls = document.querySelectorAll('[data-v-]');
		// Heuristic: 3+ Vue-scoped elements means components are hydrated
		if (vueEls.length >= 3) { resolve(true); return; }
		const app = document.getElementById('app') || document.getElementById('__nuxt');
		if (app && app.children.length > 0 && app.innerHTML.length > 100) { resolve(true); return; }
		return false;
	};
	if (check() !== false) return;
	const obs = new MutationObserver(() => { if (check() !== false) { obs.disconnect(); } });
	obs.observe(document.documentElement, { childList: true, subtree: true });
	setTimeout(() => { obs.disconnect(); resolve(true); }, 8000);
})`

// angularWaitJS waits for Angular by checking:
// 1. ng-version attribute on root element (Angular has bootstrapped)
// 2. app-root element has children
const angularWaitJS = `() => new Promise(resolve => {
	const check = () => {
		const ngRoot = document.querySelector('[ng-version]');
		if (ngRoot) { resolve(true); return; }
		const appRoot = document.querySelector('app-root');
		if (appRoot && appRoot.children.length > 0) { resolve(true); return; }
		return false;
	};
	if (check() !== false) return;
	const obs = new MutationObserver(() => { if (check() !== false) { obs.disconnect(); } });
	obs.observe(document.documentElement, { childList: true, subtree: true });
	setTimeout(() => { obs.disconnect(); resolve(true); }, 8000);
})`

// svelteWaitJS waits for Svelte/SvelteKit hydration.
const svelteWaitJS = `() => new Promise(resolve => {
	const check = () => {
		const sk = document.querySelector('[data-sveltekit-hydrate]') || document.querySelector('[data-sveltekit]');
		if (sk) { resolve(true); return; }
		// Svelte compiled components add class attributes with "s-" prefix
		const svelteEls = document.querySelectorAll('[class*="svelte-"]');
		if (svelteEls.length >= 2) { resolve(true); return; }
		return false;
	};
	if (check() !== false) return;
	const obs = new MutationObserver(() => { if (check() !== false) { obs.disconnect(); } });
	obs.observe(document.documentElement, { childList: true, subtree: true });
	setTimeout(() => { obs.disconnect(); resolve(true); }, 8000);
})`

// gatsbyWaitJS waits for Gatsby hydration.
const gatsbyWaitJS = `() => new Promise(resolve => {
	const check = () => {
		const gatsby = document.getElementById('___gatsby');
		if (gatsby && gatsby.children.length > 0 && gatsby.innerHTML.length > 200) {
			resolve(true); return;
		}
		return false;
	};
	if (check() !== false) return;
	const obs = new MutationObserver(() => { if (check() !== false) { obs.disconnect(); } });
	obs.observe(document.documentElement, { childList: true, subtree: true });
	setTimeout(() => { obs.disconnect(); resolve(true); }, 8000);
})`

// remixWaitJS waits for Remix hydration by checking __remixContext global.
const remixWaitJS = `() => new Promise(resolve => {
	const check = () => {
		if (window.__remixContext || window.__remixManifest) { resolve(true); return; }
		return false;
	};
	if (check() !== false) return;
	const interval = setInterval(() => {
		if (check() !== false) clearInterval(interval);
	}, 100);
	setTimeout(() => { clearInterval(interval); resolve(true); }, 8000);
})`

// astroWaitJS waits for Astro island hydration.
const astroWaitJS = `() => new Promise(resolve => {
	const check = () => {
		const islands = document.querySelectorAll('astro-island[ssr]');
		if (islands.length > 0) { resolve(true); return; }
		// Fallback: any astro-island that has rendered content
		const allIslands = document.querySelectorAll('astro-island');
		const hydrated = Array.from(allIslands).filter(el => el.children.length > 0);
		if (hydrated.length > 0) { resolve(true); return; }
		return false;
	};
	if (check() !== false) return;
	const obs = new MutationObserver(() => { if (check() !== false) { obs.disconnect(); } });
	obs.observe(document.documentElement, { childList: true, subtree: true });
	setTimeout(() => { obs.disconnect(); resolve(true); }, 8000);
})`

// qwikWaitJS waits for Qwik framework hydration.
const qwikWaitJS = `() => new Promise(resolve => {
	const check = () => {
		const container = document.querySelector('[q\\:container]');
		if (container) { resolve(true); return; }
		return false;
	};
	if (check() !== false) return;
	const obs = new MutationObserver(() => { if (check() !== false) { obs.disconnect(); } });
	obs.observe(document.documentElement, { childList: true, subtree: true });
	setTimeout(() => { obs.disconnect(); resolve(true); }, 8000);
})`
