package driver

import (
	"strings"
	"testing"
)

func TestEnhancedStealthJS_NilFingerprintNoPanic(t *testing.T) {
	// nil fingerprint must not panic — should use internal defaults
	defer func() {
		if r := recover(); r != nil {
			t.Errorf("EnhancedStealthJS(nil) panicked: %v", r)
		}
	}()
	js := EnhancedStealthJS(nil)
	if js == "" {
		t.Error("EnhancedStealthJS(nil) returned empty string")
	}
}

func TestEnhancedStealthJS_ContainsWebdriverEvasion(t *testing.T) {
	fp := GenerateFingerprint()
	js := EnhancedStealthJS(&fp)
	if !strings.Contains(js, "webdriver") {
		t.Error("stealth JS must patch navigator.webdriver")
	}
}

func TestEnhancedStealthJS_Contains17Techniques(t *testing.T) {
	fp := GenerateFingerprint()
	js := EnhancedStealthJS(&fp)

	// We check for distinctive markers of each of the 17 evasions.
	checks := []struct {
		name    string
		pattern string
	}{
		{"1-webdriver", "navigator.webdriver"},
		{"2-plugins", "navigator.plugins"},
		{"3-languages", "navigator.languages"},
		{"4-hardwareConcurrency", "navigator.hardwareConcurrency"},
		{"5-deviceMemory", "navigator.deviceMemory"},
		{"6-vendor", "navigator.vendor"},
		{"7-platform", "navigator.platform"},
		{"8-permissions", "Permissions.prototype.query"},
		{"9-chrome.app", "chrome.app"},
		{"10-chrome.csi", "chrome.csi"},
		{"11-chrome.loadTimes", "chrome.loadTimes"},
		{"12-chrome.runtime", "chrome.runtime"},
		{"13-WebGL", "getParameter"},
		{"14-iframe.contentWindow", "contentWindow"},
		{"15-outerDimensions", "outerWidth"},
		{"16-screen", "defineProperty(screen"},
		{"17-mediaCodecs", "MediaSource"},
	}

	for _, c := range checks {
		if !strings.Contains(js, c.pattern) {
			t.Errorf("evasion %s: expected to find %q in stealth JS", c.name, c.pattern)
		}
	}
}

func TestEnhancedStealthJS_UsesFingerprint(t *testing.T) {
	fp := GenerateFingerprint()
	js := EnhancedStealthJS(&fp)

	// The generated JS must embed the fingerprint's hardware concurrency.
	if !strings.Contains(js, "hardwareConcurrency") {
		t.Error("expected hardwareConcurrency in stealth JS")
	}

	// Vendor must appear in JS if it's non-empty.
	if fp.Vendor != "" && !strings.Contains(js, fp.Vendor) {
		t.Errorf("expected vendor %q to appear in stealth JS", fp.Vendor)
	}
}

func TestEnhancedStealthJS_LanguagesSliceEmbedded(t *testing.T) {
	fp := GenerateFingerprint()
	fp.Languages = []string{"fr-FR", "fr"}
	js := EnhancedStealthJS(&fp)
	if !strings.Contains(js, "fr-FR") {
		t.Error("custom language 'fr-FR' should be embedded in stealth JS")
	}
}
