package driver

import (
	"strings"
	"testing"
)

// ─── GenerateFingerprint ────────────────────────────────────────────────────

func TestGenerateFingerprint_FieldsNotEmpty(t *testing.T) {
	fp := GenerateFingerprint()

	if fp.UserAgent == "" {
		t.Error("UserAgent must not be empty")
	}
	if fp.Platform == "" {
		t.Error("Platform must not be empty")
	}
	if fp.Vendor == "" {
		t.Error("Vendor must not be empty")
	}
	if fp.SecChUa == "" {
		t.Error("SecChUa must not be empty")
	}
	if len(fp.Languages) == 0 {
		t.Error("Languages must not be empty")
	}
	if fp.WebGLVendor == "" {
		t.Error("WebGLVendor must not be empty")
	}
	if fp.WebGLRenderer == "" {
		t.Error("WebGLRenderer must not be empty")
	}
}

func TestGenerateFingerprint_ViewportSane(t *testing.T) {
	fp := GenerateFingerprint()

	if fp.ViewportWidth <= 0 {
		t.Errorf("ViewportWidth must be > 0, got %d", fp.ViewportWidth)
	}
	if fp.ViewportHeight <= 0 {
		t.Errorf("ViewportHeight must be > 0, got %d", fp.ViewportHeight)
	}
	if fp.ScreenWidth < fp.ViewportWidth {
		t.Errorf("ScreenWidth (%d) must be >= ViewportWidth (%d)", fp.ScreenWidth, fp.ViewportWidth)
	}
	if fp.DevicePixelRatio <= 0 {
		t.Errorf("DevicePixelRatio must be > 0, got %f", fp.DevicePixelRatio)
	}
	if fp.ColorDepth == 0 {
		t.Error("ColorDepth must not be zero")
	}
}

func TestGenerateFingerprint_HardwareRanges(t *testing.T) {
	fp := GenerateFingerprint()

	if fp.HardwareConcurrency < 2 || fp.HardwareConcurrency > 64 {
		t.Errorf("HardwareConcurrency out of realistic range: %d", fp.HardwareConcurrency)
	}
	validMemory := map[int]bool{1: true, 2: true, 4: true, 8: true, 16: true, 32: true}
	if !validMemory[fp.DeviceMemory] {
		t.Errorf("DeviceMemory not in valid set: %d", fp.DeviceMemory)
	}
}

// PlatformConsistency verifies that OS-specific fields are internally coherent.
// macOS UA must have MacIntel platform; Windows UA must have Win32.
func TestGenerateFingerprint_PlatformConsistency(t *testing.T) {
	for i := 0; i < 50; i++ {
		fp := GenerateFingerprint()
		ua := fp.UserAgent

		switch {
		case strings.Contains(ua, "Macintosh"):
			if fp.Platform != "MacIntel" {
				t.Errorf("macOS UA expects platform=MacIntel, got %q (ua=%s)", fp.Platform, ua)
			}
			if !strings.Contains(fp.WebGLVendor, "Apple") && !strings.Contains(fp.WebGLRenderer, "Apple") &&
				!strings.Contains(fp.WebGLVendor, "Google") {
				t.Errorf("macOS UA should have Apple/Google WebGL, got vendor=%q renderer=%q", fp.WebGLVendor, fp.WebGLRenderer)
			}
		case strings.Contains(ua, "Windows"):
			if fp.Platform != "Win32" {
				t.Errorf("Windows UA expects platform=Win32, got %q (ua=%s)", fp.Platform, ua)
			}
		case strings.Contains(ua, "Linux"):
			if fp.Platform != "Linux x86_64" {
				t.Errorf("Linux UA expects platform=Linux x86_64, got %q (ua=%s)", fp.Platform, ua)
			}
		default:
			t.Errorf("unrecognised OS in UA: %s", ua)
		}
	}
}

func TestGenerateFingerprint_Rotation(t *testing.T) {
	// Different calls should sometimes produce different UAs (not always identical).
	seen := map[string]bool{}
	for i := 0; i < 30; i++ {
		fp := GenerateFingerprint()
		seen[fp.UserAgent] = true
	}
	if len(seen) < 2 {
		t.Errorf("expected fingerprint rotation to produce >1 distinct UA in 30 calls, got %d", len(seen))
	}
}

// ─── AcceptLanguage ─────────────────────────────────────────────────────────

func TestBrowserFingerprint_AcceptLanguage(t *testing.T) {
	fp := GenerateFingerprint()
	lang := fp.AcceptLanguage()
	if lang == "" {
		t.Error("AcceptLanguage must not be empty")
	}
	// Should contain en-US or similar
	if !strings.Contains(lang, "-") && !strings.Contains(lang, "en") {
		t.Errorf("AcceptLanguage looks malformed: %q", lang)
	}
}

// ─── HTTPHeaders ────────────────────────────────────────────────────────────

func TestBrowserFingerprint_HTTPHeaders_RequiredKeys(t *testing.T) {
	fp := GenerateFingerprint()
	headers := fp.HTTPHeaders()

	required := []string{
		"User-Agent",
		"Accept",
		"Accept-Language",
		"Accept-Encoding",
		"Sec-Ch-Ua",
		"Sec-Ch-Ua-Mobile",
		"Sec-Ch-Ua-Platform",
		"Sec-Fetch-Dest",
		"Sec-Fetch-Mode",
		"Sec-Fetch-Site",
	}

	for _, k := range required {
		if v, ok := headers[k]; !ok || v == "" {
			t.Errorf("HTTPHeaders missing or empty key %q", k)
		}
	}
}

func TestBrowserFingerprint_HTTPHeaders_UAMatchesFingerprint(t *testing.T) {
	fp := GenerateFingerprint()
	headers := fp.HTTPHeaders()
	if headers["User-Agent"] != fp.UserAgent {
		t.Errorf("User-Agent header %q does not match fingerprint UA %q",
			headers["User-Agent"], fp.UserAgent)
	}
}

// ─── DefaultSPAWaitConfig smoke ─────────────────────────────────────────────

func TestDefaultSPAWaitConfig_Defaults(t *testing.T) {
	cfg := DefaultSPAWaitConfig()
	if cfg.OverallTimeout <= 0 {
		t.Error("OverallTimeout must be positive")
	}
	if cfg.MutationQuietPeriod <= 0 {
		t.Error("MutationQuietPeriod must be positive")
	}
	if cfg.ContentPlateauSamples <= 0 {
		t.Error("ContentPlateauSamples must be positive")
	}
	if cfg.ContentPlateauInterval <= 0 {
		t.Error("ContentPlateauInterval must be positive")
	}
}
