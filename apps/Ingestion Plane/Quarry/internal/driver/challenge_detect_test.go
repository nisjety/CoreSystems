package driver

import (
	"testing"
)

// ─── DetectChallenge ────────────────────────────────────────────────────────

func TestDetectChallenge_None_NormalHTML(t *testing.T) {
	html := `<html><body><h1>Welcome</h1><p>Normal page content here.</p></body></html>`
	got := DetectChallenge(200, html)
	if got != ChallengeNone {
		t.Errorf("expected ChallengeNone for normal HTML, got %s", got)
	}
}

func TestDetectChallenge_RateLimit_HTTP429(t *testing.T) {
	got := DetectChallenge(429, "Too Many Requests")
	if got != ChallengeRateLimit {
		t.Errorf("expected ChallengeRateLimit for HTTP 429, got %s", got)
	}
}

func TestDetectChallenge_Cloudflare_JustAMoment(t *testing.T) {
	html := `<html><head><title>Just a moment...</title></head><body>
		<p>Checking your browser before accessing the site. Cloudflare Ray ID: abc123</p>
	</body></html>`
	got := DetectChallenge(503, html)
	if got != ChallengeCloudflare {
		t.Errorf("expected ChallengeCloudflare for CF interstitial, got %s", got)
	}
}

func TestDetectChallenge_Cloudflare_Turnstile(t *testing.T) {
	html := `<html><body>
		<div class="cf-turnstile" data-sitekey="0x4AAAAAAADnPIDROrmt1Wwj"></div>
		<script src="https://challenges.cloudflare.com/turnstile/v0/api.js"></script>
	</body></html>`
	// Turnstile can appear on 200 pages
	got := DetectChallenge(200, html)
	if got != ChallengeCloudflare {
		t.Errorf("expected ChallengeCloudflare for Turnstile JS, got %s", got)
	}
}

func TestDetectChallenge_Cloudflare_CFChlOpt(t *testing.T) {
	html := `<html><body><script>window._cf_chl_opt={cType:'managed'}</script></body></html>`
	got := DetectChallenge(403, html)
	if got != ChallengeCloudflare {
		t.Errorf("expected ChallengeCloudflare for cf_chl_opt, got %s", got)
	}
}

func TestDetectChallenge_DataDome(t *testing.T) {
	html := `<html><body>
		<script src="https://js.datadome.co/tags.js"></script>
		<script>window.ddjskey = "abc123"</script>
	</body></html>`
	got := DetectChallenge(200, html)
	if got != ChallengeDataDome {
		t.Errorf("expected ChallengeDataDome, got %s", got)
	}
}

func TestDetectChallenge_PerimeterX(t *testing.T) {
	html := `<html><body>
		<script>window._pxhd = "abc123/def456";</script>
	</body></html>`
	got := DetectChallenge(200, html)
	if got != ChallengePerimeter {
		t.Errorf("expected ChallengePerimeter for _pxhd, got %s", got)
	}
}

func TestDetectChallenge_reCAPTCHA(t *testing.T) {
	html := `<html><body>
		<div class="g-recaptcha" data-sitekey="abc123"></div>
		<script src="https://www.google.com/recaptcha/api.js"></script>
	</body></html>`
	got := DetectChallenge(403, html)
	if got != ChallengeCaptcha {
		t.Errorf("expected ChallengeCaptcha for reCAPTCHA, got %s", got)
	}
}

func TestDetectChallenge_hCaptcha(t *testing.T) {
	html := `<html><body>
		<div class="h-captcha" data-sitekey="abc123"></div>
		<script src="https://hcaptcha.com/1/api.js"></script>
	</body></html>`
	got := DetectChallenge(403, html)
	if got != ChallengeCaptcha {
		t.Errorf("expected ChallengeCaptcha for hCaptcha, got %s", got)
	}
}

// ─── IsChallenge ────────────────────────────────────────────────────────────

func TestIsChallenge_TrueForCFPage(t *testing.T) {
	html := `<html><head><title>Just a moment...</title></head><body>Cloudflare Ray ID: xyz</body></html>`
	if !IsChallenge(503, html) {
		t.Error("IsChallenge should return true for CF interstitial")
	}
}

func TestIsChallenge_FalseForNormalPage(t *testing.T) {
	html := `<html><body><h1>Welcome to Example</h1><p>This is a normal page.</p></body></html>`
	if IsChallenge(200, html) {
		t.Error("IsChallenge should return false for normal page")
	}
}

// ─── ChallengeType.String() ─────────────────────────────────────────────────

func TestChallengeType_String(t *testing.T) {
	cases := []struct {
		ct   ChallengeType
		want string
	}{
		{ChallengeNone, "none"},
		{ChallengeCloudflare, "cloudflare"},
		{ChallengeAkamai, "akamai"},
		{ChallengeDataDome, "datadome"},
		{ChallengePerimeter, "perimeterx"},
		{ChallengeCaptcha, "captcha"},
		{ChallengeRateLimit, "rate-limit"},
	}
	for _, tc := range cases {
		if got := tc.ct.String(); got != tc.want {
			t.Errorf("ChallengeType(%d).String() = %q, want %q", tc.ct, got, tc.want)
		}
	}
}
