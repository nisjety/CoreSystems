package driver

import (
	"fmt"
	"strings"
)

// EnhancedStealthJS returns a comprehensive JavaScript string that patches
// headless Chrome detection vectors. It implements 17 evasion techniques
// based on puppeteer-extra-plugin-stealth, parameterised by the given
// BrowserFingerprint for internal consistency.
//
// When fp is nil, reasonable static defaults are used (Chrome 131 / macOS).
func EnhancedStealthJS(fp *BrowserFingerprint) string {
	if fp == nil {
		tmp := GenerateFingerprint()
		fp = &tmp
	}

	langJS := "['en-US','en']"
	if len(fp.Languages) > 0 {
		quoted := make([]string, len(fp.Languages))
		for i, l := range fp.Languages {
			quoted[i] = fmt.Sprintf("'%s'", l)
		}
		langJS = "[" + strings.Join(quoted, ",") + "]"
	}

	return fmt.Sprintf(`
// ── Enhanced Stealth – 17 evasion techniques ────────────────────────────

// 1. navigator.webdriver → undefined
Object.defineProperty(navigator, 'webdriver', { get: () => undefined });

// 2. navigator.plugins – inject realistic plugin array
Object.defineProperty(navigator, 'plugins', {
	get: () => {
		const p = { 0: { type: 'application/x-google-chrome-pdf', suffixes: 'pdf', description: 'Portable Document Format' },
					 length: 1, item: (i) => p[i], namedItem: (n) => p[0], refresh: () => {} };
		return p;
	}
});

// 3. navigator.languages
Object.defineProperty(navigator, 'languages', { get: () => %s });

// 4. navigator.hardwareConcurrency
Object.defineProperty(navigator, 'hardwareConcurrency', { get: () => %d });

// 5. navigator.deviceMemory
Object.defineProperty(navigator, 'deviceMemory', { get: () => %d });

// 6. navigator.vendor
Object.defineProperty(navigator, 'vendor', { get: () => '%s' });

// 7. navigator.platform
Object.defineProperty(navigator, 'platform', { get: () => '%s' });

// 8. navigator.permissions.query – notification permission patch
const origQuery = window.Permissions && Permissions.prototype.query;
if (origQuery) {
	Permissions.prototype.query = (params) => {
		if (params.name === 'notifications') {
			return Promise.resolve({ state: Notification.permission });
		}
		return origQuery.call(navigator.permissions, params);
	};
}

// 9. chrome.app mock
if (!window.chrome) window.chrome = {};
if (!window.chrome.app) {
	window.chrome.app = {
		isInstalled: false,
		InstallState: { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' },
		RunningState: { CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' },
		getDetails: () => null,
		getIsInstalled: () => false,
	};
}

// 10. chrome.csi mock
if (!window.chrome.csi) {
	window.chrome.csi = () => ({
		startE: Date.now(),
		onloadT: Date.now(),
		pageT: Math.random() * 3000 + 500,
		tran: 15,
	});
}

// 11. chrome.loadTimes mock
if (!window.chrome.loadTimes) {
	window.chrome.loadTimes = () => ({
		commitLoadTime: Date.now() / 1000,
		connectionInfo: 'h2',
		finishDocumentLoadTime: Date.now() / 1000,
		finishLoadTime: Date.now() / 1000,
		firstPaintAfterLoadTime: 0,
		firstPaintTime: Date.now() / 1000,
		navigationType: 'Other',
		npnNegotiatedProtocol: 'h2',
		requestTime: Date.now() / 1000 - 0.3,
		startLoadTime: Date.now() / 1000 - 0.5,
		wasAlternateProtocolAvailable: false,
		wasFetchedViaSpdy: true,
		wasNpnNegotiated: true,
	});
}

// 12. chrome.runtime mock (connected extension indicator)
if (!window.chrome.runtime) {
	window.chrome.runtime = {
		OnInstalledReason: { CHROME_UPDATE: 'chrome_update', INSTALL: 'install', SHARED_MODULE_UPDATE: 'shared_module_update', UPDATE: 'update' },
		OnRestartRequiredReason: { APP_UPDATE: 'app_update', OS_UPDATE: 'os_update', PERIODIC: 'periodic' },
		PlatformArch: { ARM: 'arm', MIPS: 'mips', MIPS64: 'mips64', X86_32: 'x86-32', X86_64: 'x86-64' },
		PlatformNaclArch: { ARM: 'arm', MIPS: 'mips', MIPS64: 'mips64', X86_32: 'x86-32', X86_64: 'x86-64' },
		PlatformOs: { ANDROID: 'android', CROS: 'cros', LINUX: 'linux', MAC: 'mac', OPENBSD: 'openbsd', WIN: 'win' },
		RequestUpdateCheckStatus: { NO_UPDATE: 'no_update', THROTTLED: 'throttled', UPDATE_AVAILABLE: 'update_available' },
		connect: () => ({ onDisconnect: { addListener: () => {} }, onMessage: { addListener: () => {} }, postMessage: () => {}, disconnect: () => {} }),
		sendMessage: () => {},
		id: undefined,
	};
}

// 13. WebGL vendor/renderer spoof
const getParam = WebGLRenderingContext.prototype.getParameter;
WebGLRenderingContext.prototype.getParameter = function(p) {
	if (p === 37445) return '%s'; // UNMASKED_VENDOR_WEBGL
	if (p === 37446) return '%s'; // UNMASKED_RENDERER_WEBGL
	return getParam.call(this, p);
};
if (typeof WebGL2RenderingContext !== 'undefined') {
	const getParam2 = WebGL2RenderingContext.prototype.getParameter;
	WebGL2RenderingContext.prototype.getParameter = function(p) {
		if (p === 37445) return '%s';
		if (p === 37446) return '%s';
		return getParam2.call(this, p);
	};
}

// 14. iframe.contentWindow bypass – prevent detection via cross-origin
// iframe fingerprinting (empty contentWindow in headless).
try {
	const iframeProto = HTMLIFrameElement.prototype;
	const origContentWindow = Object.getOwnPropertyDescriptor(iframeProto, 'contentWindow');
	if (origContentWindow) {
		Object.defineProperty(iframeProto, 'contentWindow', {
			get: function() {
				const w = origContentWindow.get.call(this);
				if (w && !w.chrome) { w.chrome = window.chrome; }
				return w;
			}
		});
	}
} catch(e) {}

// 15. window.outerWidth / outerHeight – headless often has 0
Object.defineProperty(window, 'outerWidth', { get: () => %d });
Object.defineProperty(window, 'outerHeight', { get: () => %d });

// 16. screen dimensions + colorDepth + pixelDepth
Object.defineProperty(screen, 'width', { get: () => %d });
Object.defineProperty(screen, 'height', { get: () => %d });
Object.defineProperty(screen, 'availWidth', { get: () => %d });
Object.defineProperty(screen, 'availHeight', { get: () => %d - 40 }); // taskbar offset
Object.defineProperty(screen, 'colorDepth', { get: () => %d });
Object.defineProperty(screen, 'pixelDepth', { get: () => %d });

// 17. Media codecs – ensure common codecs report supported
if (typeof MediaSource !== 'undefined') {
	const origIsTypeSupported = MediaSource.isTypeSupported;
	MediaSource.isTypeSupported = function(type) {
		const common = ['video/mp4', 'video/webm', 'audio/webm', 'audio/mp4'];
		if (common.some(c => type.startsWith(c))) return true;
		return origIsTypeSupported.call(this, type);
	};
}
`,
		langJS,
		fp.HardwareConcurrency,
		fp.DeviceMemory,
		fp.Vendor,
		fp.Platform,
		fp.WebGLVendor, fp.WebGLRenderer,
		fp.WebGLVendor, fp.WebGLRenderer,
		fp.ViewportWidth,
		fp.ViewportHeight,
		fp.ScreenWidth,
		fp.ScreenHeight,
		fp.ScreenWidth,
		fp.ScreenHeight,
		fp.ColorDepth,
		fp.ColorDepth,
	)
}
