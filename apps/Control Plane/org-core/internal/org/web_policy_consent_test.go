package org

import (
	"encoding/json"
	"testing"
)

// W3 E3.2-f: per-origin consent defaults in the org web policy document.

// TestOriginConsentKeysAreNotInTheCatalogueYet fails the day Quarry's catalogue
// gains the consent keys and the vendored copy is refreshed: delete org-core's own
// table (webOriginConsentKind) then and let the catalogue rows decide.
func TestOriginConsentKeysAreNotInTheCatalogueYet(t *testing.T) {
	for _, key := range OriginConsentKeys() {
		if _, known := DefaultWebPolicyCatalogue().Spec(key); known {
			t.Fatalf("%s is now a catalogue row: retire webOriginConsentKind", key)
		}
	}
}

func TestValidateAcceptsOriginConsentDefaults(t *testing.T) {
	for _, raw := range []string{
		`{"caps":{"browser.consent.origin":"disabled"}}`,
		`{"caps":{"browser.consent.origin":"allowed"}}`,
		`{"caps":{"browser.consent.max_scope":"turn"}}`,
		`{"caps":{"browser.consent.max_scope":"conversation"}}`,
		`{"caps":{"browser.consent.max_scope":"always"}}`,
		`{"caps":{"browser.act.upload":"disabled","browser.consent.max_scope":"conversation"}}`,
	} {
		if err := mustDoc(t, raw).Validate(nil); err != nil {
			t.Fatalf("%s: %v", raw, err)
		}
	}
}

func TestValidateRefusesIllegalOriginConsentValues(t *testing.T) {
	for _, tc := range []struct {
		raw string
		key string
	}{
		{`{"caps":{"browser.consent.origin":"forced"}}`, WebCapOriginConsent},
		{`{"caps":{"browser.consent.origin":"turn"}}`, WebCapOriginConsent},
		{`{"caps":{"browser.consent.max_scope":"once"}}`, WebCapOriginConsentMaxScope},
		{`{"caps":{"browser.consent.max_scope":"disabled"}}`, WebCapOriginConsentMaxScope},
		{`{"caps":{"browser.consent.max_scope":"Turn"}}`, WebCapOriginConsentMaxScope},
		{`{"caps":{"browser.consent.max_scope":""}}`, WebCapOriginConsentMaxScope},
	} {
		wantWebPolicyError(t, mustDoc(t, tc.raw).Validate(nil), ErrIllegalMode, tc.key)
	}
}

// A near miss of a consent key is still an unknown capability.
func TestValidateRefusesUnknownConsentKey(t *testing.T) {
	doc := mustDoc(t, `{"caps":{"browser.consent.origins":"disabled"}}`)
	wantWebPolicyError(t, doc.Validate(nil), ErrUnknownCapability, "browser.consent.origins")
}

func TestTightenedRanksOriginConsentDefaults(t *testing.T) {
	cases := []struct {
		name          string
		before, after string
		want          bool
	}{
		{"nothing to turn", `{}`, `{"caps":{"browser.consent.max_scope":"turn"}}`, true},
		{"always to conversation", `{"caps":{"browser.consent.max_scope":"always"}}`, `{"caps":{"browser.consent.max_scope":"conversation"}}`, true},
		{"conversation to turn", `{"caps":{"browser.consent.max_scope":"conversation"}}`, `{"caps":{"browser.consent.max_scope":"turn"}}`, true},
		{"turn to conversation loosens", `{"caps":{"browser.consent.max_scope":"turn"}}`, `{"caps":{"browser.consent.max_scope":"conversation"}}`, false},
		{"turn to nothing loosens", `{"caps":{"browser.consent.max_scope":"turn"}}`, `{}`, false},
		{"always stated equals nothing stated", `{}`, `{"caps":{"browser.consent.max_scope":"always"}}`, false},
		{"consent disabled", `{}`, `{"caps":{"browser.consent.origin":"disabled"}}`, true},
		{"consent re-allowed loosens", `{"caps":{"browser.consent.origin":"disabled"}}`, `{"caps":{"browser.consent.origin":"allowed"}}`, false},
		{"mixed change", `{"caps":{"browser.consent.max_scope":"turn"}}`, `{"caps":{"browser.consent.max_scope":"always","browser.consent.origin":"disabled"}}`, false},
	}
	for _, tc := range cases {
		if got := Tightened(mustDoc(t, tc.before), mustDoc(t, tc.after)); got != tc.want {
			t.Fatalf("%s: Tightened=%v want %v", tc.name, got, tc.want)
		}
	}
}

func TestChangedKeysNamesOriginConsentKeys(t *testing.T) {
	got := ChangedKeys(mustDoc(t, `{}`), mustDoc(t, `{"caps":{"browser.consent.max_scope":"turn","browser.consent.origin":"disabled"}}`))
	want := []string{WebCapOriginConsentMaxScope, WebCapOriginConsent}
	if !equalStrings(got, want) {
		t.Fatalf("ChangedKeys=%v want %v", got, want)
	}
	if !equalStrings(OriginConsentKeys(), want) {
		t.Fatalf("OriginConsentKeys=%v want %v (sorted)", OriginConsentKeys(), want)
	}
}

// The entitlement fold leaves the consent defaults as stated, so the claim Auth
// Core mints carries them in caps beside the other modes.
func TestEffectiveCarriesOriginConsentDefaultsIntoTheClaimCaps(t *testing.T) {
	doc := mustDoc(t, `{"caps":{"browser.consent.max_scope":"conversation","browser.consent.origin":"allowed"}}`)
	effective := doc.Effective(WebEntitlements{PaidSearch: true})
	if effective.Caps[WebCapOriginConsentMaxScope] != WebConsentScopeConversation || effective.Caps[WebCapOriginConsent] != WebModeAllowed {
		t.Fatalf("effective caps=%v", effective.Caps)
	}
	canonical, err := effective.Canonical()
	if err != nil {
		t.Fatal(err)
	}
	var decoded map[string]map[string]string
	if err := json.Unmarshal(canonical, &decoded); err != nil {
		t.Fatalf("decode %s: %v", canonical, err)
	}
	if decoded["caps"][WebCapOriginConsentMaxScope] != WebConsentScopeConversation {
		t.Fatalf("canonical=%s", canonical)
	}
}
