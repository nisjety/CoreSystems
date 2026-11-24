package users

import (
	"regexp"
	"strings"
	"time"
)

// Preference contract v2 helpers (WS16-S2). Pure functions over the
// user_settings maps so the HTTP layer stays thin. Storage is unchanged:
// categories "language" and "appearance" in user_settings.

const (
	// PreferenceAuto is the stored word meaning "follow the locale".
	PreferenceAuto = "auto"

	DefaultLanguage   = "en-US"
	DefaultDateFormat = "MM/DD/YYYY"
	DefaultTimeFormat = "12h"
)

var languagePattern = regexp.MustCompile(`^[a-z]{2,3}(-[A-Z]{2})?$`)

// ValidDateFormats is the closed set accepted for dateFormat.
var ValidDateFormats = map[string]bool{"MM/DD/YYYY": true, "DD/MM/YYYY": true, "YYYY-MM-DD": true}

// ValidTimeFormats is the closed set accepted for timeFormat.
var ValidTimeFormats = map[string]bool{"12h": true, "24h": true}

// IsValidTimezone reports whether name is an IANA zone name. "Local" and the
// empty string are rejected; a bare country code such as "US" is not a zone.
func IsValidTimezone(name string) bool {
	if name == "" || name == "Local" || len(name) > 64 {
		return false
	}
	_, err := time.LoadLocation(name)
	return err == nil
}

// NormalizeLanguage validates a BCP-47-ish language tag (ll or ll-CC, max 10
// chars) and returns it unchanged when valid.
func NormalizeLanguage(lang string) (string, bool) {
	lang = strings.TrimSpace(lang)
	if len(lang) == 0 || len(lang) > 10 || !languagePattern.MatchString(lang) {
		return "", false
	}
	return lang, true
}

// legacyCountryCode is the country-code region the legacy default carried ("US"). The legacy language route still
// accepts it, because older clients read it back and send it again.
var legacyCountryCode = regexp.MustCompile(`^[A-Z]{2}$`)

// LegacyLanguagePatch validates the fields of a legacy PUT /api/v1/settings/language body and returns the patch to
// store. It applies the same rules as the preference contract (WS16-S2). A field the body leaves out is empty and is
// not written: the stored row merges by key, so writing "" would blank a value the body never meant to touch. On a
// rejected field it returns the error message to send and false.
func LegacyLanguagePatch(language, region, dateFormat, timeFormat string) (map[string]any, string, bool) {
	patch := map[string]any{}
	if language != "" {
		lang, ok := NormalizeLanguage(language)
		if !ok {
			return nil, "language must be a language tag such as nb-NO", false
		}
		patch["language"] = lang
	}
	if region != "" {
		if !IsValidTimezone(region) && !legacyCountryCode.MatchString(region) {
			return nil, "region must be an IANA zone name", false
		}
		patch["region"] = region
	}
	if dateFormat != "" {
		if !ValidDateFormats[dateFormat] {
			return nil, "dateFormat must be MM/DD/YYYY, DD/MM/YYYY or YYYY-MM-DD", false
		}
		patch["dateFormat"] = dateFormat
	}
	if timeFormat != "" {
		if !ValidTimeFormats[timeFormat] {
			return nil, "timeFormat must be 12h or 24h", false
		}
		patch["timeFormat"] = timeFormat
	}
	return patch, "", true
}

// ResolveTimezone reads the timezone from the language category map: the
// "timezone" key first, then the legacy "region" value only when it is a valid
// IANA zone. It never returns a country code such as "US".
func ResolveTimezone(lang map[string]any) (string, bool) {
	for _, key := range []string{"timezone", "region"} {
		if v, ok := lang[key].(string); ok && IsValidTimezone(v) {
			return v, true
		}
	}
	return "", false
}

// ExplicitKeys returns the set of keys (from keys) that are present in the
// stored row. A nil row means nothing is explicit.
func ExplicitKeys(rawRow map[string]any, keys ...string) map[string]bool {
	out := make(map[string]bool, len(keys))
	for _, k := range keys {
		_, ok := rawRow[k]
		out[k] = ok
	}
	return out
}

// isExplicitChoice is true when a stored value exists and is not "auto".
func isExplicitChoice(raw map[string]any, key string) bool {
	v, ok := raw[key]
	if !ok || v == nil {
		return false
	}
	if s, isStr := v.(string); isStr && (s == PreferenceAuto || s == "") {
		return false
	}
	return true
}

// BuildPreferenceView assembles the GET /api/v1/preferences "preferences"
// object. appearance and lang are the merged (defaults + stored) maps;
// rawAppearance and rawLang are the stored rows only (nil when absent).
func BuildPreferenceView(appearance, lang, rawAppearance, rawLang map[string]any, crawlIngestMode any, notifEmail, notifPush bool) map[string]any {
	language := DefaultLanguage
	if v, ok := lang["language"].(string); ok && v != "" {
		language = v
	}
	dateFormat := DefaultDateFormat
	if v, ok := lang["dateFormat"].(string); ok && ValidDateFormats[v] {
		dateFormat = v
	}
	timeFormat := DefaultTimeFormat
	if v, ok := lang["timeFormat"].(string); ok && ValidTimeFormats[v] {
		timeFormat = v
	}
	compact, _ := appearance["compactMode"].(bool)

	tz, hasTZ := ResolveTimezone(rawLang)
	explicit := map[string]bool{
		"language":    isExplicitChoice(rawLang, "language"),
		"timezone":    hasTZ,
		"dateFormat":  isExplicitChoice(rawLang, "dateFormat") && ValidDateFormats[dateFormat],
		"timeFormat":  isExplicitChoice(rawLang, "timeFormat") && ValidTimeFormats[timeFormat],
		"compactMode": isExplicitChoice(rawAppearance, "compactMode"),
	}

	view := map[string]any{
		"theme":           appearance["theme"],
		"language":        language,
		"dateFormat":      dateFormat,
		"timeFormat":      timeFormat,
		"compactMode":     compact,
		"crawlIngestMode": crawlIngestMode,
		"notifications": map[string]bool{
			"email": notifEmail,
			"push":  notifPush,
		},
		"explicit": explicit,
	}
	if hasTZ {
		view["timezone"] = tz
	}
	return view
}
