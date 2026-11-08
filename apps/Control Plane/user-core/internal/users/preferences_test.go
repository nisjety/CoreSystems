package users

import "testing"

// WS16-S2 preference contract v2: pure helpers over the user_settings maps.

func TestResolveTimezonePrefersKeyThenValidLegacyRegion(t *testing.T) {
	cases := []struct {
		name   string
		lang   map[string]any
		want   string
		wantOK bool
	}{
		{"timezone key wins over region", map[string]any{"timezone": "Europe/Oslo", "region": "America/New_York"}, "Europe/Oslo", true},
		{"valid legacy region is used", map[string]any{"region": "Asia/Tokyo"}, "Asia/Tokyo", true},
		{"invalid timezone falls through to valid region", map[string]any{"timezone": "Not/AZone", "region": "Europe/Berlin"}, "Europe/Berlin", true},
		{"country code region is never a zone", map[string]any{"region": "US"}, "", false},
		{"Local is rejected", map[string]any{"timezone": "Local"}, "", false},
		{"empty row", map[string]any{}, "", false},
		{"nil row", nil, "", false},
		{"non-string timezone ignored", map[string]any{"timezone": 5}, "", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, ok := ResolveTimezone(tc.lang)
			if ok != tc.wantOK || got != tc.want {
				t.Fatalf("ResolveTimezone(%v) = (%q, %v), want (%q, %v)", tc.lang, got, ok, tc.want, tc.wantOK)
			}
			if len(got) == 2 {
				t.Fatalf("ResolveTimezone returned a country code %q", got)
			}
		})
	}
}

func TestNormalizeLanguageAcceptsShortTagsOnly(t *testing.T) {
	for _, ok := range []string{"en", "en-US", "nb", "nb-NO", "fil", " en-US "} {
		got, valid := NormalizeLanguage(ok)
		if !valid {
			t.Fatalf("NormalizeLanguage(%q) rejected a valid tag", ok)
		}
		if got != trimSpace(ok) {
			t.Fatalf("NormalizeLanguage(%q) = %q, want the trimmed tag", ok, got)
		}
	}
	for _, bad := range []string{"", "   ", "e", "english", "en_US", "EN-us", "en-USA", "en-US-x-private", "zh-Hans-CN", "123", "en-us"} {
		if got, valid := NormalizeLanguage(bad); valid || got != "" {
			t.Fatalf("NormalizeLanguage(%q) = (%q, %v), want rejection", bad, got, valid)
		}
	}
}

func trimSpace(s string) string {
	for len(s) > 0 && s[0] == ' ' {
		s = s[1:]
	}
	for len(s) > 0 && s[len(s)-1] == ' ' {
		s = s[:len(s)-1]
	}
	return s
}

func TestBuildPreferenceViewOmitsTimezoneWhenUnset(t *testing.T) {
	appearance := map[string]any{"theme": "light", "compactMode": false}
	lang := map[string]any{"language": "en-US", "dateFormat": "MM/DD/YYYY", "timeFormat": "12h"}
	view := BuildPreferenceView(appearance, lang, nil, nil, "auto", true, true)
	if _, present := view["timezone"]; present {
		t.Fatalf("timezone must be omitted when unset, got %v", view["timezone"])
	}
	explicit := view["explicit"].(map[string]bool)
	for k, v := range explicit {
		if v {
			t.Fatalf("explicit[%s] = true for a brand new user", k)
		}
	}
	if len(explicit) != 5 {
		t.Fatalf("explicit keys = %v, want language, timezone, dateFormat, timeFormat, compactMode", explicit)
	}
	if view["dateFormat"] != DefaultDateFormat || view["timeFormat"] != DefaultTimeFormat || view["language"] != DefaultLanguage {
		t.Fatalf("defaults not applied: %v", view)
	}
}

func TestBuildPreferenceViewExplicitTracksStoredKeysOnly(t *testing.T) {
	appearance := map[string]any{"theme": "dark", "compactMode": true}
	lang := map[string]any{"language": "nb-NO", "dateFormat": "DD/MM/YYYY", "timeFormat": "24h", "timezone": "Europe/Oslo"}
	view := BuildPreferenceView(appearance, lang, map[string]any{"compactMode": true}, lang, "never", false, true)
	if view["timezone"] != "Europe/Oslo" {
		t.Fatalf("timezone = %v", view["timezone"])
	}
	explicit := view["explicit"].(map[string]bool)
	for _, k := range []string{"language", "timezone", "dateFormat", "timeFormat", "compactMode"} {
		if !explicit[k] {
			t.Fatalf("explicit[%s] = false, want true when stored", k)
		}
	}
	// Merged values alone (no stored row) are never explicit.
	merged := BuildPreferenceView(appearance, lang, nil, nil, "never", false, true)
	for k, v := range merged["explicit"].(map[string]bool) {
		if v {
			t.Fatalf("explicit[%s] = true without a stored row", k)
		}
	}
	notif := view["notifications"].(map[string]bool)
	if notif["email"] || !notif["push"] {
		t.Fatalf("notifications = %v", notif)
	}
}

func TestBuildPreferenceViewAutoFormatsAreNotExplicit(t *testing.T) {
	lang := map[string]any{"language": "en-US", "dateFormat": "auto", "timeFormat": "auto"}
	view := BuildPreferenceView(map[string]any{"theme": "light"}, lang, nil, lang, "auto", true, true)
	explicit := view["explicit"].(map[string]bool)
	if explicit["dateFormat"] || explicit["timeFormat"] {
		t.Fatalf("auto date/time format must not be explicit: %v", explicit)
	}
	// The effective value falls back to the default rather than echoing "auto".
	if view["dateFormat"] != DefaultDateFormat || view["timeFormat"] != DefaultTimeFormat {
		t.Fatalf("auto must resolve to defaults, got %v / %v", view["dateFormat"], view["timeFormat"])
	}
	if !explicit["language"] {
		t.Fatalf("a stored language is explicit")
	}
}

func TestExplicitKeysReportsPresenceOnly(t *testing.T) {
	got := ExplicitKeys(map[string]any{"a": 1, "b": nil}, "a", "b", "c")
	if !got["a"] || !got["b"] || got["c"] {
		t.Fatalf("ExplicitKeys = %v", got)
	}
	if got := ExplicitKeys(nil, "a"); got["a"] {
		t.Fatalf("nil row has no explicit keys")
	}
}

func TestIsValidTimezoneRejectsNonZones(t *testing.T) {
	for _, ok := range []string{"UTC", "Europe/Oslo", "America/New_York"} {
		if !IsValidTimezone(ok) {
			t.Fatalf("%q should be valid", ok)
		}
	}
	for _, bad := range []string{"", "Local", "US", "Nowhere/Land"} {
		if IsValidTimezone(bad) {
			t.Fatalf("%q should be invalid", bad)
		}
	}
}
