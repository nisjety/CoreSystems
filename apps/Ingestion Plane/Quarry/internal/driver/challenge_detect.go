package driver

import (
	"strings"
)

// ChallengeType identifies the kind of bot protection challenge detected.
type ChallengeType int

const (
	ChallengeNone       ChallengeType = iota
	ChallengeCloudflare               // Cloudflare "Just a moment..." interstitial
	ChallengeAkamai                   // Akamai Bot Manager
	ChallengeDataDome                 // DataDome challenge page
	ChallengePerimeter                // PerimeterX / HUMAN challenge
	ChallengeCaptcha                  // Generic CAPTCHA (reCAPTCHA, hCaptcha, Turnstile)
	ChallengeRateLimit                // HTTP 429 or explicit rate-limit page
)

// String returns a human-readable name for the challenge type.
func (c ChallengeType) String() string {
	switch c {
	case ChallengeCloudflare:
		return "cloudflare"
	case ChallengeAkamai:
		return "akamai"
	case ChallengeDataDome:
		return "datadome"
	case ChallengePerimeter:
		return "perimeterx"
	case ChallengeCaptcha:
		return "captcha"
	case ChallengeRateLimit:
		return "rate-limit"
	default:
		return "none"
	}
}

// DetectChallenge examines an HTTP response body and status code to determine
// if the page is a bot-protection challenge rather than real content.
// Returns ChallengeNone if no challenge is detected.
func DetectChallenge(statusCode int, body string) ChallengeType {
	lower := strings.ToLower(body)

	// Rate limiting (HTTP 429).
	if statusCode == 429 {
		return ChallengeRateLimit
	}

	// Cloudflare challenge (403/503 with distinctive markers).
	if statusCode == 403 || statusCode == 503 {
		if containsAny(lower,
			"just a moment",
			"checking your browser",
			"cf-browser-verification",
			"cf_chl_opt",
			"cloudflare ray id",
			"challenge-platform",
			"/cdn-cgi/challenge-platform/",
		) {
			return ChallengeCloudflare
		}
	}

	// Cloudflare Turnstile (can appear on 200 pages too).
	if containsAny(lower,
		"challenges.cloudflare.com/turnstile",
		"cf-turnstile",
	) {
		return ChallengeCloudflare
	}

	// Akamai Bot Manager.
	if containsAny(lower,
		"akamai",
		"_bm_sz",
		"ak_bmsc",
		"akamai-bot-manager",
	) && (statusCode == 403 || strings.Contains(lower, "access denied")) {
		return ChallengeAkamai
	}

	// DataDome.
	if containsAny(lower,
		"datadome",
		"dd.js",
		"datadome.co/captcha",
	) {
		return ChallengeDataDome
	}

	// PerimeterX / HUMAN.
	if containsAny(lower,
		"perimeterx",
		"_pxhd",
		"px-captcha",
		"human security",
	) {
		return ChallengePerimeter
	}

	// Generic CAPTCHA (reCAPTCHA, hCaptcha).
	if statusCode == 403 || statusCode == 503 {
		if containsAny(lower,
			"recaptcha",
			"g-recaptcha",
			"hcaptcha",
			"h-captcha",
		) {
			return ChallengeCaptcha
		}
	}

	return ChallengeNone
}

// IsChallenge returns true if the response appears to be a bot challenge
// rather than real content.
func IsChallenge(statusCode int, body string) bool {
	return DetectChallenge(statusCode, body) != ChallengeNone
}

// containsAny returns true if s contains any of the substrings.
func containsAny(s string, substrings ...string) bool {
	for _, sub := range substrings {
		if strings.Contains(s, sub) {
			return true
		}
	}
	return false
}
