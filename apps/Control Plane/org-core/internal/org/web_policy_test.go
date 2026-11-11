package org

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// webPolicyCataloguePinPath is the sha256 pin beside the vendored catalogue copy.
const (
	webPolicyCataloguePath       = "testdata/web-policy-catalogue.v1.json"
	webPolicyCataloguePinPath    = "testdata/web-policy-catalogue.v1.json.sha256"
	webPolicyQuarryCataloguePath = "../../../../Ingestion Plane/Quarry-v2/crates/quarry-core/contracts/web-policy-catalogue.v1.json"
)

func webPolicySHA256LF(data []byte) string {
	normalised := bytes.ReplaceAll(data, []byte("\r\n"), []byte("\n"))
	sum := sha256.Sum256(normalised)
	return hex.EncodeToString(sum[:])
}

func mustDoc(t *testing.T, raw string) WebPolicyDoc {
	t.Helper()
	var doc WebPolicyDoc
	if err := json.Unmarshal([]byte(raw), &doc); err != nil {
		t.Fatalf("decode %s: %v", raw, err)
	}
	return doc
}

func wantWebPolicyError(t *testing.T, err error, kind error, key string) {
	t.Helper()
	if err == nil {
		t.Fatalf("want %v on %q, got no error", kind, key)
	}
	var refusal *WebPolicyError
	if !errors.As(err, &refusal) {
		t.Fatalf("want a *WebPolicyError, got %T %v", err, err)
	}
	if !errors.Is(err, kind) {
		t.Fatalf("error %v is not %v", err, kind)
	}
	if refusal.Key != key {
		t.Fatalf("refusal key=%q want %q (%v)", refusal.Key, key, err)
	}
}

// --- catalogue pin ---

// TestWebPolicyCataloguePinMatchesTheEmbeddedCopy fails when the vendored catalogue
// was edited without refreshing its pin, or the pin without the catalogue.
func TestWebPolicyCataloguePinMatchesTheEmbeddedCopy(t *testing.T) {
	pin, err := os.ReadFile(webPolicyCataloguePinPath)
	if err != nil {
		t.Fatalf("read the catalogue pin: %v", err)
	}
	fields := strings.Fields(string(pin))
	if len(fields) == 0 {
		t.Fatal("the catalogue pin is empty")
	}
	if actual := webPolicySHA256LF(WebPolicyCatalogueBytes()); fields[0] != actual {
		t.Fatalf("the embedded catalogue differs from its pin: pinned %s, actual %s; refresh it from the Quarry source, never by hand", fields[0], actual)
	}
	onDisk, err := os.ReadFile(webPolicyCataloguePath)
	if err != nil {
		t.Fatalf("read the catalogue copy: %v", err)
	}
	if webPolicySHA256LF(onDisk) != fields[0] {
		t.Fatalf("the catalogue file on disk differs from the embedded bytes and the pin")
	}
}

// TestWebPolicyCatalogueCopyIsByteIdenticalToTheQuarrySource compares the copy with
// the file Quarry owns. The source is outside this module, so a missing source skips
// the test unless REQUIRE_CONTRACT is set; the pin test above is what a Docker build
// context relies on.
func TestWebPolicyCatalogueCopyIsByteIdenticalToTheQuarrySource(t *testing.T) {
	source, err := os.ReadFile(filepath.FromSlash(webPolicyQuarryCataloguePath))
	if err != nil {
		if os.IsNotExist(err) && os.Getenv("REQUIRE_CONTRACT") == "" {
			t.Skip("the Quarry catalogue source is not in this build context (set REQUIRE_CONTRACT=1 to make that a failure)")
		}
		t.Fatalf("read the Quarry catalogue source: %v", err)
	}
	if webPolicySHA256LF(source) != webPolicySHA256LF(WebPolicyCatalogueBytes()) {
		t.Fatal("the vendored catalogue differs from quarry-core/contracts/web-policy-catalogue.v1.json; copy the new bytes and its pin")
	}
}

func TestWebPolicyCatalogueLoadsAndIsTheV1Shape(t *testing.T) {
	catalogue := DefaultWebPolicyCatalogue()
	if catalogue.Version != 1 || len(catalogue.Keys) != 15 {
		t.Fatalf("catalogue version=%d keys=%d, want version 1 with 15 keys", catalogue.Version, len(catalogue.Keys))
	}
	for key, kind := range map[string]string{
		"browser.act.script": "ability", "search.paid_provider": "ability",
		"robots.max_mode": "ceiling", "proxy.max_tier": "ceiling",
		"domains.allow": "rules", "domains.deny": "rules",
	} {
		spec, ok := catalogue.Spec(key)
		if !ok || spec.Kind != kind {
			t.Fatalf("catalogue key %s = %+v (known %v), want kind %s", key, spec, ok, kind)
		}
	}
}

func TestLoadWebPolicyCatalogueRefusesWhatItDoesNotUnderstand(t *testing.T) {
	cases := map[string]string{
		"unknown field": `{"version":1,"keys":[],"extra":1}`,
		"wrong version": `{"version":2,"keys":[]}`,
		"duplicate key": `{"version":1,"keys":[{"key":"a","kind":"ability"},{"key":"a","kind":"ability"}]}`,
		"unknown kind":  `{"version":1,"keys":[{"key":"a","kind":"mystery"}]}`,
		"empty key":     `{"version":1,"keys":[{"key":"","kind":"ability"}]}`,
		"not json":      `not json`,
	}
	for name, raw := range cases {
		if _, err := LoadWebPolicyCatalogue([]byte(raw)); err == nil {
			t.Errorf("%s: a catalogue this build cannot read was accepted", name)
		}
	}
}

// --- Validate ---

func TestValidateRejectsForcedOnAbility(t *testing.T) {
	doc := WebPolicyDoc{Caps: map[string]string{"browser.act.upload": "forced"}}
	wantWebPolicyError(t, doc.Validate(nil), ErrIllegalMode, "browser.act.upload")
	for _, mode := range []string{"", "on", "Disabled", "block"} {
		doc := WebPolicyDoc{Caps: map[string]string{"browser.act.upload": mode}}
		wantWebPolicyError(t, doc.Validate(nil), ErrIllegalMode, "browser.act.upload")
	}
	for _, mode := range []string{"disabled", "allowed"} {
		doc := WebPolicyDoc{Caps: map[string]string{"browser.act.upload": mode}}
		if err := doc.Validate(nil); err != nil {
			t.Fatalf("%s on an ability refused: %v", mode, err)
		}
	}
}

func TestValidateProtectionModesAreTheMirrorOfAbilities(t *testing.T) {
	catalogue, err := LoadWebPolicyCatalogue([]byte(`{"version":1,"keys":[{"key":"safety.example","kind":"protection","applies_to":"all","baseline":"allowed","unlockable":false,"reserved":false,"since":"test","site":null,"zdr_implies_disabled":false}]}`))
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	wantWebPolicyError(t, WebPolicyDoc{Caps: map[string]string{"safety.example": "disabled"}}.Validate(catalogue), ErrIllegalMode, "safety.example")
	for _, mode := range []string{"allowed", "forced"} {
		if err := (WebPolicyDoc{Caps: map[string]string{"safety.example": mode}}).Validate(catalogue); err != nil {
			t.Fatalf("%s on a protection refused: %v", mode, err)
		}
	}
}

func TestValidateRejectsUnknownKey(t *testing.T) {
	for _, key := range []string{"browser.act.teleport", "", "BROWSER.ACT.UPLOAD", "browser.act.upload "} {
		doc := WebPolicyDoc{Caps: map[string]string{key: "disabled"}}
		wantWebPolicyError(t, doc.Validate(nil), ErrUnknownCapability, key)
	}
}

func TestValidateKeepsCeilingsAndDomainRulesOutOfCaps(t *testing.T) {
	for _, key := range []string{"robots.max_mode", "proxy.max_tier", "domains.allow", "domains.deny"} {
		doc := WebPolicyDoc{Caps: map[string]string{key: "disabled"}}
		wantWebPolicyError(t, doc.Validate(nil), ErrIllegalMode, key)
	}
	wantWebPolicyError(t, WebPolicyDoc{RobotsMax: "always"}.Validate(nil), ErrIllegalMode, "robots.max_mode")
	wantWebPolicyError(t, WebPolicyDoc{ProxyMax: "tor"}.Validate(nil), ErrIllegalMode, "proxy.max_tier")
	for _, ok := range []WebPolicyDoc{{RobotsMax: "strict"}, {RobotsMax: "respect"}, {RobotsMax: "ignore"}, {ProxyMax: "none"}, {ProxyMax: "pool"}} {
		if err := ok.Validate(nil); err != nil {
			t.Fatalf("%+v refused: %v", ok, err)
		}
	}
}

func TestValidateRejectsIPLiteralAndWildcard(t *testing.T) {
	bad := []string{
		"*.example.com", "*", "example.*", "192.168.0.1", "10.0.0.1", "8.8.8.8", "[::1]", "::1",
		"2001:db8::1", "example.com:8080", "example.com.", ".example.com", "exa mple.com",
		"example..com", "-example.com", "example-.com", "Example.COM", "http://example.com",
		"example.com/path", "ex_ample.com", "user@example.com", "123", "1.2.3.4.5", "münchen.de",
	}
	for _, host := range bad {
		for _, list := range []string{"allow", "deny"} {
			doc := WebPolicyDoc{Domains: &WebPolicyDomains{}}
			if list == "allow" {
				doc.Domains.Allow = []string{host}
			} else {
				doc.Domains.Deny = []string{host}
			}
			wantWebPolicyError(t, doc.Validate(nil), ErrInvalidDomain, "domains."+list)
		}
	}
	for _, host := range []string{"example.com", "a.b.example.co.uk", "localhost", "xn--mnchen-3ya.de", "a-b.example.com", "3d.example.com"} {
		doc := WebPolicyDoc{Domains: &WebPolicyDomains{Deny: []string{host}}}
		if err := doc.Validate(nil); err != nil {
			t.Fatalf("%s refused: %v", host, err)
		}
	}
}

func TestNormalizeCanonicalisesHostsSoUpperCaseIsAcceptedOnWrite(t *testing.T) {
	doc := WebPolicyDoc{Domains: &WebPolicyDomains{Deny: []string{" Example.COM ", "example.com", "B.example.com"}}}
	normal := doc.Normalize()
	if got := strings.Join(normal.Domains.Deny, ","); got != "b.example.com,example.com" {
		t.Fatalf("normalised deny=%q", got)
	}
	if err := normal.Validate(nil); err != nil {
		t.Fatalf("normalised document refused: %v", err)
	}
	if doc.Validate(nil) == nil {
		t.Fatal("the raw upper-case document must not validate: Validate checks what it is given")
	}
	if again := normal.Normalize(); again.Digest() != normal.Digest() {
		t.Fatal("Normalize is not idempotent")
	}
}

func TestValidateRejectsTooManyOrTooLargeRules(t *testing.T) {
	many := make([]string, 0, 65)
	for i := 0; i < 65; i++ {
		many = append(many, fmt.Sprintf("h%d.example.com", i))
	}
	wantWebPolicyError(t, WebPolicyDoc{Domains: &WebPolicyDomains{Deny: many}}.Validate(nil), ErrWebPolicyTooLarge, "domains")
	wantWebPolicyError(t, WebPolicyDoc{Domains: &WebPolicyDomains{Allow: many[:40], Deny: many[40:]}}.Validate(nil), ErrWebPolicyTooLarge, "domains")
	if err := (WebPolicyDoc{Domains: &WebPolicyDomains{Deny: many[:64]}}).Validate(nil); err != nil {
		t.Fatalf("exactly 64 short rules refused: %v", err)
	}

	// Fewer than 64 rules and under 2048 bytes of rules, but over the document cap.
	long := make([]string, 0, 26)
	for i := 0; i < 26; i++ {
		long = append(long, strings.Repeat("a", 60)+fmt.Sprintf("%02d", i)+".example.com")
	}
	err := WebPolicyDoc{Domains: &WebPolicyDomains{Deny: long}}.Validate(nil)
	wantWebPolicyError(t, err, ErrWebPolicyTooLarge, "")
}

func TestWebPolicyMaxBytesLeavesRoomForTheClaimEnvelope(t *testing.T) {
	// The issuer adds {"v":1,"rev":"<12 hex>", to the document and fails closed above
	// 2048 bytes in total, and Effective can add a disabled paid search afterwards.
	envelope := len(`{"v":1,"rev":"abcdef012345",`)
	fold := len(`"caps":{"search.paid_provider":"disabled"},`)
	if WebPolicyMaxBytes+envelope+fold > 2048 {
		t.Fatalf("a maximal document, its entitlement fold and its claim envelope (%d + %d + %d) would exceed the issuer's 2 KiB cap", WebPolicyMaxBytes, fold, envelope)
	}
}

// TestEffectiveOfAMaximalDocumentStaysUnderTheClaimCap builds the largest document
// Validate accepts on a plan with no entitlements and checks the claim the issuer
// would mint from its effective form.
func TestEffectiveOfAMaximalDocumentStaysUnderTheClaimCap(t *testing.T) {
	deny := make([]string, 0, 30)
	for i := 0; i < 30; i++ {
		deny = append(deny, fmt.Sprintf("%s%02d.example.com", strings.Repeat("b", 40), i))
	}
	doc := WebPolicyDoc{RobotsMax: "ignore", ProxyMax: "pool", Domains: &WebPolicyDomains{Deny: deny}}
	for len(doc.Domains.Deny) > 0 {
		if doc.Validate(nil) == nil {
			break
		}
		doc.Domains.Deny = doc.Domains.Deny[:len(doc.Domains.Deny)-1]
	}
	if err := doc.Validate(nil); err != nil || len(doc.Domains.Deny) < 20 {
		t.Fatalf("could not build a near-maximal valid document: %d rules, err %v", len(doc.Domains.Deny), err)
	}
	effective := doc.Effective(WebEntitlements{})
	body, err := json.Marshal(map[string]any{"v": 1, "rev": "abcdef012345", "caps": effective.Caps, "robots_max": effective.RobotsMax, "proxy_max": effective.ProxyMax, "domains": effective.Domains})
	if err != nil {
		t.Fatalf("encode claim: %v", err)
	}
	if len(body) > 2048 {
		t.Fatalf("the claim for a maximal document is %d bytes, over the issuer's 2048 cap", len(body))
	}
}

// --- Digest ---

func TestDigestIsStableUnderKeyOrder(t *testing.T) {
	a := mustDoc(t, `{"caps":{"browser.act.upload":"disabled","search.paid_provider":"disabled"},"robots_max":"respect","proxy_max":"none","domains":{"deny":["b.example.com","a.example.com"],"allow":["z.example.com","y.example.com"]}}`)
	b := mustDoc(t, `{"domains":{"allow":["y.example.com","z.example.com"],"deny":["a.example.com","b.example.com","a.example.com"]},"proxy_max":"none","robots_max":"respect","caps":{"search.paid_provider":"disabled","browser.act.upload":"disabled"}}`)
	if a.Digest() != b.Digest() {
		t.Fatalf("digest changed with key or list order: %s vs %s", a.Digest(), b.Digest())
	}
	canonicalA, _ := a.Canonical()
	canonicalB, _ := b.Canonical()
	if !bytes.Equal(canonicalA, canonicalB) {
		t.Fatalf("canonical forms differ:\n%s\n%s", canonicalA, canonicalB)
	}
	changed := a.Clone()
	changed.Caps["browser.act.upload"] = "allowed"
	if changed.Digest() == a.Digest() {
		t.Fatal("a different document produced the same digest")
	}
}

func TestDigestIsTwelveHexAndPinned(t *testing.T) {
	digest := (WebPolicyDoc{}).Digest()
	if len(digest) != 12 || strings.Trim(digest, "0123456789abcdef") != "" {
		t.Fatalf("digest %q is not 12 lower-case hex digits", digest)
	}
	// sha256("{}") = 44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a
	if digest != "44136fa355b3" {
		t.Fatalf("the empty document digest = %s; the canonical form changed, which would change every stored rev", digest)
	}
	known := mustDoc(t, `{"caps":{"browser.act.upload":"disabled"}}`)
	sum := sha256.Sum256([]byte(`{"caps":{"browser.act.upload":"disabled"}}`))
	if known.Digest() != hex.EncodeToString(sum[:])[:12] {
		t.Fatalf("digest is not the first 12 hex of the sha256 of the canonical JSON: %s", known.Digest())
	}
}

// --- Effective and entitlements ---

func TestEffectiveFoldsEntitlements(t *testing.T) {
	unlocked := WebPolicyDoc{Caps: map[string]string{"search.paid_provider": "allowed"}, RobotsMax: "ignore", ProxyMax: "pool"}
	none := unlocked.Effective(WebEntitlements{})
	if none.RobotsMax != "respect" || none.ProxyMax != "none" || none.Caps["search.paid_provider"] != "disabled" {
		t.Fatalf("a plan with no entitlements must fold every unlock away: %+v", none)
	}
	all := unlocked.Effective(WebEntitlements{RobotsOverride: true, ProxyPool: true, PaidSearch: true})
	if all.RobotsMax != "ignore" || all.ProxyMax != "pool" || all.Caps["search.paid_provider"] != "allowed" {
		t.Fatalf("an entitled plan keeps the stored unlocks: %+v", all)
	}
	partial := unlocked.Effective(WebEntitlements{ProxyPool: true, PaidSearch: true})
	if partial.RobotsMax != "respect" || partial.ProxyMax != "pool" {
		t.Fatalf("each entitlement gates only its own unlock: %+v", partial)
	}

	// Tightening only: a strict ceiling is not raised to respect, and nothing the
	// document disabled is re-enabled by an entitlement.
	tight := WebPolicyDoc{Caps: map[string]string{"search.paid_provider": "disabled", "browser.act.upload": "disabled"}, RobotsMax: "strict", ProxyMax: "none"}
	got := tight.Effective(WebEntitlements{RobotsOverride: true, ProxyPool: true, PaidSearch: true})
	if got.RobotsMax != "strict" || got.ProxyMax != "none" || got.Caps["search.paid_provider"] != "disabled" || got.Caps["browser.act.upload"] != "disabled" {
		t.Fatalf("an entitlement loosened a stored restriction: %+v", got)
	}
	// An organization with no document still gets the plan-derived restriction.
	empty := WebPolicyDoc{}.Effective(WebEntitlements{})
	if empty.Caps["search.paid_provider"] != "disabled" || empty.RobotsMax != "" || empty.ProxyMax != "" {
		t.Fatalf("empty document on a bare plan: %+v", empty)
	}
	if !(WebPolicyDoc{}).Effective(WebEntitlements{PaidSearch: true}).IsEmpty() {
		t.Fatal("an entitled organization with no document must have an empty effective document")
	}
	// Effective never mutates its receiver.
	if unlocked.RobotsMax != "ignore" || unlocked.Caps["search.paid_provider"] != "allowed" {
		t.Fatalf("Effective mutated the stored document: %+v", unlocked)
	}
}

func TestCheckEntitledNamesTheMissingCapability(t *testing.T) {
	cases := []struct {
		doc  WebPolicyDoc
		ent  WebEntitlements
		want string
	}{
		{WebPolicyDoc{RobotsMax: "ignore"}, WebEntitlements{ProxyPool: true, PaidSearch: true}, WebRobotsOverrideEntitlementKey},
		{WebPolicyDoc{ProxyMax: "pool"}, WebEntitlements{RobotsOverride: true, PaidSearch: true}, WebProxyPoolEntitlementKey},
		{WebPolicyDoc{Caps: map[string]string{"search.paid_provider": "allowed"}}, WebEntitlements{RobotsOverride: true, ProxyPool: true}, WebPaidSearchEntitlementKey},
	}
	for _, tc := range cases {
		err := tc.doc.CheckEntitled(tc.ent)
		var refusal *WebPolicyError
		if !errors.As(err, &refusal) || !errors.Is(err, ErrNotEntitled) || refusal.Capability != tc.want {
			t.Errorf("%+v: err=%v want capability %s", tc.doc, err, tc.want)
		}
		if tc.doc.CheckEntitled(WebEntitlements{RobotsOverride: true, ProxyPool: true, PaidSearch: true}) != nil {
			t.Errorf("%+v refused for a fully entitled plan", tc.doc)
		}
	}
	// Tightening is never gated.
	tightening := WebPolicyDoc{RobotsMax: "strict", ProxyMax: "none", Caps: map[string]string{"search.paid_provider": "disabled", "browser.act.upload": "disabled"}}
	if err := tightening.CheckEntitled(WebEntitlements{}); err != nil {
		t.Fatalf("a tightening document needs no entitlement: %v", err)
	}
	if err := (WebPolicyDoc{RobotsMax: "respect", ProxyMax: "none"}).CheckEntitled(WebEntitlements{}); err != nil {
		t.Fatalf("the baseline ceilings need no entitlement: %v", err)
	}
}

func TestWebEntitlementsAreComputedFromThePlan(t *testing.T) {
	want := map[string][3]bool{
		"free":       {false, false, false},
		"trial":      {false, false, true},
		"hobby":      {false, false, true},
		"standard":   {false, false, true},
		"pro":        {false, true, true},
		"enterprise": {true, true, true},
		" PRO ":      {false, true, true},
		"unknown":    {false, false, false},
		"":           {false, false, false},
	}
	for plan, expected := range want {
		got := WebEntitlementsForPlan(plan)
		if got.RobotsOverride != expected[0] || got.ProxyPool != expected[1] || got.PaidSearch != expected[2] {
			t.Errorf("plan %q = %+v want %v", plan, got, expected)
		}
	}
	if PlanAllowsWebCapability("enterprise", "web.something_else") {
		t.Fatal("an unknown entitlement key must not be allowed")
	}
	keys := WebEntitlementsForPlan("pro").Map()
	if len(keys) != 3 || !keys["web.proxy_pool"] || keys["web.robots_override"] || !keys["web.paid_search"] {
		t.Fatalf("entitlement map = %v", keys)
	}
}

// --- Tightened ---

func TestTightenedDetectsOnlyTightening(t *testing.T) {
	empty := WebPolicyDoc{}
	cases := []struct {
		name     string
		from, to string
		want     bool
	}{
		{"empty to empty", `{}`, `{}`, false},
		{"identical", `{"caps":{"browser.act.upload":"disabled"}}`, `{"caps":{"browser.act.upload":"disabled"}}`, false},
		{"disable an ability", `{}`, `{"caps":{"browser.act.upload":"disabled"}}`, true},
		{"re-enable an ability", `{"caps":{"browser.act.upload":"disabled"}}`, `{}`, false},
		{"allowed to disabled", `{"caps":{"browser.act.upload":"allowed"}}`, `{"caps":{"browser.act.upload":"disabled"}}`, true},
		{"explicit allowed is no change", `{}`, `{"caps":{"browser.act.upload":"allowed"}}`, false},
		{"lower robots ceiling", `{"robots_max":"ignore"}`, `{"robots_max":"respect"}`, true},
		{"state a robots ceiling", `{}`, `{"robots_max":"respect"}`, true},
		{"raise robots ceiling", `{"robots_max":"respect"}`, `{"robots_max":"ignore"}`, false},
		{"drop a robots ceiling", `{"robots_max":"strict"}`, `{}`, false},
		{"lower proxy ceiling", `{"proxy_max":"pool"}`, `{"proxy_max":"none"}`, true},
		{"raise proxy ceiling", `{"proxy_max":"none"}`, `{"proxy_max":"pool"}`, false},
		{"add an allow list", `{}`, `{"domains":{"allow":["a.example.com"]}}`, true},
		{"shrink an allow list", `{"domains":{"allow":["a.example.com","b.example.com"]}}`, `{"domains":{"allow":["a.example.com"]}}`, true},
		{"grow an allow list", `{"domains":{"allow":["a.example.com"]}}`, `{"domains":{"allow":["a.example.com","b.example.com"]}}`, false},
		{"swap an allow entry", `{"domains":{"allow":["a.example.com"]}}`, `{"domains":{"allow":["b.example.com"]}}`, false},
		{"remove the allow list", `{"domains":{"allow":["a.example.com"]}}`, `{}`, false},
		{"grow a deny list", `{}`, `{"domains":{"deny":["bad.example.com"]}}`, true},
		{"shrink a deny list", `{"domains":{"deny":["a.example.com","b.example.com"]}}`, `{"domains":{"deny":["a.example.com"]}}`, false},
		{"swap a deny entry", `{"domains":{"deny":["a.example.com"]}}`, `{"domains":{"deny":["b.example.com"]}}`, false},
		{"tighten one, loosen another", `{"caps":{"browser.act.upload":"disabled"}}`, `{"caps":{"browser.act.input":"disabled"}}`, false},
		{"tighten two", `{}`, `{"caps":{"browser.act.upload":"disabled"},"proxy_max":"none"}`, true},
		{"tighten caps, raise ceiling", `{"proxy_max":"none"}`, `{"caps":{"browser.act.upload":"disabled"},"proxy_max":"pool"}`, false},
		{"order and case do not matter", `{"domains":{"deny":["B.example.com","a.example.com"]}}`, `{"domains":{"deny":["a.example.com","b.example.com"]}}`, false},
	}
	for _, tc := range cases {
		from, to := mustDoc(t, tc.from), mustDoc(t, tc.to)
		if got := Tightened(from, to); got != tc.want {
			t.Errorf("%s: Tightened(%s, %s) = %v want %v", tc.name, tc.from, tc.to, got, tc.want)
		}
	}
	if Tightened(empty, empty) {
		t.Fatal("two empty documents are not a tightening")
	}
}

func TestChangedKeysCarriesKeysOnly(t *testing.T) {
	from := mustDoc(t, `{"caps":{"browser.act.upload":"disabled"},"robots_max":"respect","domains":{"deny":["secret-customer.example.com"]}}`)
	to := mustDoc(t, `{"caps":{"browser.act.input":"disabled"},"proxy_max":"none","domains":{"deny":["secret-customer.example.com"],"allow":["other.example.com"]}}`)
	keys := ChangedKeys(from, to)
	want := []string{"browser.act.input", "browser.act.upload", "domains.allow", "proxy.max_tier", "robots.max_mode"}
	if strings.Join(keys, ",") != strings.Join(want, ",") {
		t.Fatalf("changed keys=%v want %v", keys, want)
	}
	for _, key := range keys {
		if strings.Contains(key, "example.com") {
			t.Fatalf("a changed key carries a host name: %s", key)
		}
	}
	if len(ChangedKeys(from, from)) != 0 {
		t.Fatal("an unchanged document reported changed keys")
	}
}

// --- audit payload ---

func TestWebPolicyAuditPayloadIsContentFree(t *testing.T) {
	change := webPolicyAuditChange{
		OrgID: "org-1", ActorID: "user-1", ActorRole: "admin", Revision: 3, Rev: "abcdef012345", Tightened: true,
		Keys: ChangedKeys(WebPolicyDoc{}, mustDoc(t, `{"caps":{"browser.act.upload":"disabled"},"domains":{"deny":["secret-customer.example.com"]}}`)),
	}
	eventID, payload := webPolicyAuditPayload(change, time.Date(2026, 11, 2, 12, 0, 0, 0, time.UTC))
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("encode payload: %v", err)
	}
	if strings.Contains(string(body), "secret-customer") || strings.Contains(string(body), ".example.com") {
		t.Fatalf("the audit payload carries a host name: %s", body)
	}
	if payload["event_id"] != eventID || payload["org_id"] != "org-1" || payload["producer"] != "org-core" || payload["plane"] != "control" || payload["event"] != WebPolicyAuditEvent {
		t.Fatalf("payload identity = %v", payload)
	}
	details, _ := payload["details"].(map[string]any)
	if details["revision"] != int64(3) || details["tightened"] != true || details["rev"] != "abcdef012345" {
		t.Fatalf("payload details = %v", details)
	}
	if keys, _ := details["keys"].([]string); strings.Join(keys, ",") != "browser.act.upload,domains.deny" {
		t.Fatalf("payload keys = %v", details["keys"])
	}
	if len(eventID) > 128 || !strings.HasPrefix(eventID, "webpolicy:org-core:") {
		t.Fatalf("event id %q breaks the outbox's id constraint", eventID)
	}
	other, _ := webPolicyAuditPayload(webPolicyAuditChange{OrgID: "org-1", ActorID: "u", ActorRole: "admin", Revision: 4}, time.Now())
	if other == eventID {
		t.Fatal("two revisions share an event id")
	}
	if WebPolicyAuditSubject != "verevon.audit.v2.control.org-core.org_web_policy_changed" {
		t.Fatalf("audit subject = %s", WebPolicyAuditSubject)
	}
}

// --- migrations ---

func readWebPolicyMigration(t *testing.T, name string) string {
	t.Helper()
	body, err := os.ReadFile(filepath.Join("..", "..", "migrations", name))
	if err != nil {
		t.Fatalf("read %s: %v", name, err)
	}
	return strings.ReplaceAll(string(body), "\r\n", "\n")
}

func TestWebPolicyMigrationsPinRLSLeastPrivilegeAndCascade(t *testing.T) {
	storage := readWebPolicyMigration(t, "025_organization_web_policy.up.sql")
	for _, required := range []string{
		"CREATE TABLE IF NOT EXISTS organization_web_policy",
		"org_id     TEXT        PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE",
		"CHECK (revision > 0)",
		"rev_digest",
		"REVOKE ALL ON organization_web_policy FROM org_core_app",
		"GRANT SELECT, INSERT, UPDATE ON organization_web_policy TO org_core_app",
		"ALTER TABLE organization_web_policy ENABLE ROW LEVEL SECURITY",
		"USING (org_id = current_setting('app.current_org', true))",
		"WITH CHECK (org_id = current_setting('app.current_org', true))",
	} {
		if !strings.Contains(storage, required) {
			t.Errorf("025 is missing %q", required)
		}
	}
	history := readWebPolicyMigration(t, "026_organization_web_policy_history.up.sql")
	for _, required := range []string{
		"PRIMARY KEY (org_id, revision)",
		"REFERENCES organizations(id) ON DELETE CASCADE",
		"tightened  BOOLEAN     NOT NULL",
		"REVOKE ALL ON organization_web_policy_history FROM org_core_app",
		"GRANT SELECT, INSERT ON organization_web_policy_history TO org_core_app",
		"ENABLE ROW LEVEL SECURITY",
		"app.current_org",
	} {
		if !strings.Contains(history, required) {
			t.Errorf("026 is missing %q", required)
		}
	}
	if strings.Contains(history, "UPDATE ON organization_web_policy_history") || strings.Contains(history, "DELETE ON organization_web_policy_history") {
		t.Error("026 grants the runtime role more than SELECT and INSERT on the append-only history")
	}
	outbox := readWebPolicyMigration(t, "027_organization_web_policy_audit_outbox.up.sql")
	for _, required := range []string{
		"organization_web_policy_audit_outbox",
		"verevon.audit.v2.control.org-core.org_web_policy_changed",
		"payload->>'event_id' = event_id AND payload->>'org_id' = org_id",
		"REVOKE ALL ON organization_web_policy_audit_outbox FROM org_core_app",
		"GRANT SELECT, INSERT ON organization_web_policy_audit_outbox TO org_core_app",
		"ENABLE ROW LEVEL SECURITY",
	} {
		if !strings.Contains(outbox, required) {
			t.Errorf("027 is missing %q", required)
		}
	}
	for _, name := range []string{"025_organization_web_policy", "026_organization_web_policy_history", "027_organization_web_policy_audit_outbox"} {
		down := readWebPolicyMigration(t, name+".down.sql")
		if !strings.Contains(down, "DROP TABLE IF EXISTS") {
			t.Errorf("%s.down.sql does not drop its table idempotently", name)
		}
	}
	// Re-runnable: every CREATE is IF NOT EXISTS and every policy is dropped first.
	for name, sql := range map[string]string{"025": storage, "026": history, "027": outbox} {
		if strings.Count(sql, "CREATE TABLE") != strings.Count(sql, "CREATE TABLE IF NOT EXISTS") {
			t.Errorf("%s has a CREATE TABLE that is not IF NOT EXISTS", name)
		}
		if strings.Count(sql, "CREATE POLICY") != strings.Count(sql, "DROP POLICY IF EXISTS") {
			t.Errorf("%s creates a policy it does not drop first", name)
		}
	}
}
