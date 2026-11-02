package org

import (
	"bytes"
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
)

// Web policy (WS7.H): the org layer of Quarry's tighten-only web policy.
//
// org-core stores one document per organization. It names capabilities to
// disable, ceilings to lower (robots, proxy) and host names to deny or allow. It
// is the same shape as the web_policy claim Auth Core mints (WS7.I) and that the
// Quarry edge reads (quarry-edge auth.rs parse_web_policy_claim), so the stored
// document is the claim body without its `v` and `rev`. It cannot loosen anything:
// the edge folds it with a meet, so a mode the catalogue forbids is refused here
// and a mode the plan does not entitle is folded away by Effective.
//
// The catalogue of capability keys is the one Quarry owns
// (crates/quarry-core/contracts/web-policy-catalogue.v1.json). A byte-identical
// copy is vendored under testdata/ and embedded, with a sha256 pin and a test that
// compares it with the source, so a key added there fails here until this copy is
// refreshed.

//go:embed testdata/web-policy-catalogue.v1.json
var webPolicyCatalogueBytes []byte

// WebPolicyMaxBytes caps the canonical document. The issuer's claim adds `v` and
// `rev` (about 30 bytes) and fails closed above 2 KiB, so the document stops short
// of 2048 to leave that room.
const WebPolicyMaxBytes = 2000

// WebPolicyMaxDomainEntries and WebPolicyMaxDomainBytes are the edge's own limits
// on a layer's host rules (quarry-core web_policy MAX_RULE_ENTRIES, MAX_RULE_BYTES).
const (
	WebPolicyMaxDomainEntries = 64
	WebPolicyMaxDomainBytes   = 2048
)

// The computed entitlement keys. Like feature.zero_data_retention they are never
// stored: they are derived from the organization's plan on every read.
const (
	WebRobotsOverrideEntitlementKey = "web.robots_override"
	WebProxyPoolEntitlementKey      = "web.proxy_pool"
	WebPaidSearchEntitlementKey     = "web.paid_search"
)

// webCapPaidSearch is the capability key the paid-search entitlement gates.
const webCapPaidSearch = "search.paid_provider"

// Modes and ceilings an admin may state.
const (
	WebModeDisabled = "disabled"
	WebModeAllowed  = "allowed"
	WebModeForced   = "forced"

	webRobotsStrict  = "strict"
	webRobotsRespect = "respect"
	webRobotsIgnore  = "ignore"

	webProxyNone = "none"
	webProxyPool = "pool"
)

var (
	// ErrRevisionConflict: the If-Match revision is not the stored one.
	ErrRevisionConflict = errors.New("web policy revision conflict")
	// ErrIllegalMode: a mode, ceiling or key placement the catalogue forbids.
	ErrIllegalMode = errors.New("illegal web policy mode")
	// ErrUnknownCapability: a capability key the catalogue does not have.
	ErrUnknownCapability = errors.New("unknown web policy capability")
	// ErrInvalidDomain: a domain rule that is not a plain lowercase host name.
	ErrInvalidDomain = errors.New("invalid web policy domain")
	// ErrWebPolicyTooLarge: more than 64 rules, 2 KiB of rules or a document over the cap.
	ErrWebPolicyTooLarge = errors.New("web policy too large")
	// ErrNotEntitled: the document unlocks something the organization's plan does not.
	ErrNotEntitled = errors.New("web policy capability not entitled")
)

// WebPolicyError is a refusal of a document, with the key it concerns.
type WebPolicyError struct {
	Kind    error
	Key     string
	Message string
	// Capability is the entitlement key for ErrNotEntitled.
	Capability string
}

func (e *WebPolicyError) Error() string {
	if e.Message != "" {
		return e.Kind.Error() + ": " + e.Message
	}
	return e.Kind.Error()
}

func (e *WebPolicyError) Unwrap() error { return e.Kind }

// RevisionConflictError carries the stored revision a stale writer must re-read.
type RevisionConflictError struct {
	Current int64
}

func (e *RevisionConflictError) Error() string {
	return fmt.Sprintf("%s: current revision is %d", ErrRevisionConflict.Error(), e.Current)
}

func (e *RevisionConflictError) Unwrap() error { return ErrRevisionConflict }

// WebCapSpec is one row of the catalogue.
type WebCapSpec struct {
	Key                string `json:"key"`
	Kind               string `json:"kind"`
	AppliesTo          string `json:"applies_to"`
	Baseline           string `json:"baseline"`
	Unlockable         bool   `json:"unlockable"`
	Reserved           bool   `json:"reserved"`
	ZDRImpliesDisabled bool   `json:"zdr_implies_disabled"`
	Since              string `json:"since"`
	Site               any    `json:"site"`
}

// WebPolicyCatalogue is the vendored capability catalogue.
type WebPolicyCatalogue struct {
	Version int          `json:"version"`
	Keys    []WebCapSpec `json:"keys"`
	index   map[string]WebCapSpec
}

// Spec returns the row for a key.
func (c *WebPolicyCatalogue) Spec(key string) (WebCapSpec, bool) {
	spec, ok := c.index[key]
	return spec, ok
}

// LoadWebPolicyCatalogue decodes a catalogue document strictly: a field this build
// does not know, a duplicate key or an unexpected kind is an error, so a catalogue
// that grew a concept is not silently half read.
func LoadWebPolicyCatalogue(raw []byte) (*WebPolicyCatalogue, error) {
	raw = bytes.ReplaceAll(raw, []byte("\r\n"), []byte("\n"))
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var catalogue WebPolicyCatalogue
	if err := decoder.Decode(&catalogue); err != nil {
		return nil, fmt.Errorf("decode web policy catalogue: %w", err)
	}
	if catalogue.Version != 1 {
		return nil, fmt.Errorf("web policy catalogue version %d is not supported", catalogue.Version)
	}
	catalogue.index = make(map[string]WebCapSpec, len(catalogue.Keys))
	for _, spec := range catalogue.Keys {
		if spec.Key == "" {
			return nil, errors.New("web policy catalogue has an empty key")
		}
		if _, duplicate := catalogue.index[spec.Key]; duplicate {
			return nil, fmt.Errorf("web policy catalogue repeats key %s", spec.Key)
		}
		switch spec.Kind {
		case "ability", "protection", "ceiling", "rules":
		default:
			return nil, fmt.Errorf("web policy catalogue key %s has unknown kind %q", spec.Key, spec.Kind)
		}
		catalogue.index[spec.Key] = spec
	}
	return &catalogue, nil
}

var defaultWebPolicyCatalogue = func() *WebPolicyCatalogue {
	catalogue, err := LoadWebPolicyCatalogue(webPolicyCatalogueBytes)
	if err != nil {
		// The embedded bytes are pinned by a test, so this is a build defect.
		panic(err)
	}
	return catalogue
}()

// DefaultWebPolicyCatalogue is the embedded, pinned catalogue.
func DefaultWebPolicyCatalogue() *WebPolicyCatalogue { return defaultWebPolicyCatalogue }

// WebPolicyCatalogueBytes returns a copy of the embedded catalogue bytes, for the
// pin test.
func WebPolicyCatalogueBytes() []byte {
	return append([]byte(nil), webPolicyCatalogueBytes...)
}

// WebPolicyDomains are the host rules of a document.
type WebPolicyDomains struct {
	// Allow, when non-empty, limits an autonomous run to these hosts and their
	// sub-domains. Empty means no allow rule, not "allow nothing".
	Allow []string `json:"allow,omitempty"`
	// Deny hosts (and sub-domains) are refused for every principal.
	Deny []string `json:"deny,omitempty"`
}

// WebPolicyDoc is the org layer. The zero value is the empty policy.
type WebPolicyDoc struct {
	// Caps maps an ability or protection key to its mode.
	Caps map[string]string `json:"caps,omitempty"`
	// RobotsMax is the robots.txt ceiling: strict, respect or ignore. Empty states nothing.
	RobotsMax string `json:"robots_max,omitempty"`
	// ProxyMax is the proxy tier ceiling: none or pool. Empty states nothing.
	ProxyMax string            `json:"proxy_max,omitempty"`
	Domains  *WebPolicyDomains `json:"domains,omitempty"`
}

func (d WebPolicyDoc) allow() []string {
	if d.Domains == nil {
		return nil
	}
	return d.Domains.Allow
}

func (d WebPolicyDoc) deny() []string {
	if d.Domains == nil {
		return nil
	}
	return d.Domains.Deny
}

// Clone copies the document so a caller can edit it.
func (d WebPolicyDoc) Clone() WebPolicyDoc {
	out := WebPolicyDoc{RobotsMax: d.RobotsMax, ProxyMax: d.ProxyMax}
	if len(d.Caps) > 0 {
		out.Caps = make(map[string]string, len(d.Caps))
		for key, mode := range d.Caps {
			out.Caps[key] = mode
		}
	}
	if d.Domains != nil {
		out.Domains = &WebPolicyDomains{
			Allow: append([]string(nil), d.Domains.Allow...),
			Deny:  append([]string(nil), d.Domains.Deny...),
		}
	}
	return out
}

// Normalize returns the canonical form: host names trimmed and lower-cased, the
// lists de-duplicated and sorted, and empty containers dropped. It does not
// validate. Two documents that mean the same thing normalise to the same bytes,
// which is what Digest and the history rely on.
func (d WebPolicyDoc) Normalize() WebPolicyDoc {
	out := d.Clone()
	if len(out.Caps) == 0 {
		out.Caps = nil
	}
	out.RobotsMax = strings.ToLower(strings.TrimSpace(out.RobotsMax))
	out.ProxyMax = strings.ToLower(strings.TrimSpace(out.ProxyMax))
	if out.Domains != nil {
		allow := canonicalHosts(out.Domains.Allow)
		deny := canonicalHosts(out.Domains.Deny)
		if len(allow) == 0 && len(deny) == 0 {
			out.Domains = nil
		} else {
			out.Domains = &WebPolicyDomains{Allow: allow, Deny: deny}
		}
	}
	return out
}

func canonicalHosts(hosts []string) []string {
	seen := make(map[string]struct{}, len(hosts))
	out := make([]string, 0, len(hosts))
	for _, host := range hosts {
		host = strings.ToLower(strings.TrimSpace(host))
		if _, duplicate := seen[host]; duplicate {
			continue
		}
		seen[host] = struct{}{}
		out = append(out, host)
	}
	sort.Strings(out)
	if len(out) == 0 {
		return nil
	}
	return out
}

// Canonical is the normalised document as JSON: struct fields in declared order,
// map keys sorted, no insignificant whitespace.
func (d WebPolicyDoc) Canonical() ([]byte, error) {
	return json.Marshal(d.Normalize())
}

// Digest is the revision digest the claim carries: the first 12 hex digits of the
// sha256 of the canonical JSON. It is stable under key order and list order.
func (d WebPolicyDoc) Digest() string {
	canonical, err := d.Canonical()
	if err != nil {
		// A WebPolicyDoc is strings and maps of strings; marshalling cannot fail.
		return strings.Repeat("0", 12)
	}
	sum := sha256.Sum256(canonical)
	return hex.EncodeToString(sum[:])[:12]
}

// IsEmpty reports whether the document states nothing.
func (d WebPolicyDoc) IsEmpty() bool {
	n := d.Normalize()
	return len(n.Caps) == 0 && n.RobotsMax == "" && n.ProxyMax == "" && n.Domains == nil
}

// Validate refuses a document the catalogue does not allow. It validates the
// document as given (call Normalize first to accept upper-case host names):
//   - every capability key is in the catalogue, and only ability and protection
//     keys appear in caps (ceilings and domain rules have their own fields);
//   - an ability is disabled or allowed, never forced; a protection is allowed or
//     forced, never disabled;
//   - robots_max is strict, respect or ignore; proxy_max is none or pool;
//   - a domain rule is a plain lower-case host name: no wildcard, no IP literal,
//     no port, no trailing dot;
//   - at most 64 rules and 2048 bytes of rules, and the canonical document fits
//     WebPolicyMaxBytes.
func (d WebPolicyDoc) Validate(catalogue *WebPolicyCatalogue) error {
	if catalogue == nil {
		catalogue = DefaultWebPolicyCatalogue()
	}
	keys := make([]string, 0, len(d.Caps))
	for key := range d.Caps {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	for _, key := range keys {
		spec, known := catalogue.Spec(key)
		if !known {
			return &WebPolicyError{Kind: ErrUnknownCapability, Key: key, Message: "capability " + key + " is not in the catalogue"}
		}
		mode := d.Caps[key]
		switch spec.Kind {
		case "ability":
			if mode == WebModeForced {
				return &WebPolicyError{Kind: ErrIllegalMode, Key: key, Message: "forced is not expressible on the ability " + key}
			}
			if mode != WebModeDisabled && mode != WebModeAllowed {
				return &WebPolicyError{Kind: ErrIllegalMode, Key: key, Message: key + " must be disabled or allowed"}
			}
		case "protection":
			if mode == WebModeDisabled {
				return &WebPolicyError{Kind: ErrIllegalMode, Key: key, Message: "disabled is not expressible on the protection " + key}
			}
			if mode != WebModeAllowed && mode != WebModeForced {
				return &WebPolicyError{Kind: ErrIllegalMode, Key: key, Message: key + " must be allowed or forced"}
			}
		case "ceiling":
			return &WebPolicyError{Kind: ErrIllegalMode, Key: key, Message: key + " is a ceiling: use robots_max or proxy_max"}
		default:
			return &WebPolicyError{Kind: ErrIllegalMode, Key: key, Message: key + " is a domain rule: use domains"}
		}
	}
	switch d.RobotsMax {
	case "", webRobotsStrict, webRobotsRespect, webRobotsIgnore:
	default:
		return &WebPolicyError{Kind: ErrIllegalMode, Key: "robots.max_mode", Message: "robots_max must be strict, respect or ignore"}
	}
	switch d.ProxyMax {
	case "", webProxyNone, webProxyPool:
	default:
		return &WebPolicyError{Kind: ErrIllegalMode, Key: "proxy.max_tier", Message: "proxy_max must be none or pool"}
	}
	entries, bytesUsed := 0, 0
	for _, list := range []struct {
		key   string
		hosts []string
	}{{"domains.allow", d.allow()}, {"domains.deny", d.deny()}} {
		for _, host := range list.hosts {
			if !validWebPolicyHost(host) {
				return &WebPolicyError{Kind: ErrInvalidDomain, Key: list.key, Message: "domain rules are plain lower-case host names without wildcard, IP literal or port"}
			}
			entries++
			bytesUsed += len(host)
		}
	}
	if entries > WebPolicyMaxDomainEntries || bytesUsed > WebPolicyMaxDomainBytes {
		return &WebPolicyError{Kind: ErrWebPolicyTooLarge, Key: "domains", Message: "domain rules exceed 64 entries or 2 KiB"}
	}
	canonical, err := d.Canonical()
	if err != nil {
		return fmt.Errorf("encode web policy: %w", err)
	}
	if len(canonical) > WebPolicyMaxBytes {
		return &WebPolicyError{Kind: ErrWebPolicyTooLarge, Message: fmt.Sprintf("the policy is %d bytes, the limit is %d", len(canonical), WebPolicyMaxBytes)}
	}
	return nil
}

// validWebPolicyHost mirrors quarry-core web_policy::valid_host: lower-case ASCII
// letters, digits, hyphen and dots only; no empty, over-long or hyphen-edged
// label; and a last label that is not all digits, which rules out IPv4 literals.
// IPv6 literals, wildcards and ports contain characters outside the set.
func validWebPolicyHost(host string) bool {
	if host == "" || len(host) > 253 || strings.HasPrefix(host, ".") || strings.HasSuffix(host, ".") {
		return false
	}
	for i := 0; i < len(host); i++ {
		c := host[i]
		if !((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-' || c == '.') {
			return false
		}
	}
	labels := strings.Split(host, ".")
	for _, label := range labels {
		if label == "" || len(label) > 63 || strings.HasPrefix(label, "-") || strings.HasSuffix(label, "-") {
			return false
		}
	}
	last := labels[len(labels)-1]
	allDigits := true
	for i := 0; i < len(last); i++ {
		if last[i] < '0' || last[i] > '9' {
			allDigits = false
			break
		}
	}
	return !allDigits
}

// WebEntitlements are the three computed entitlements.
type WebEntitlements struct {
	RobotsOverride bool
	ProxyPool      bool
	PaidSearch     bool
}

// Map is the wire form: entitlement key to enabled.
func (e WebEntitlements) Map() map[string]bool {
	return map[string]bool{
		WebRobotsOverrideEntitlementKey: e.RobotsOverride,
		WebProxyPoolEntitlementKey:      e.ProxyPool,
		WebPaidSearchEntitlementKey:     e.PaidSearch,
	}
}

// CheckEntitled refuses a document that unlocks a capability the plan lacks:
// robots_max=ignore needs web.robots_override, proxy_max=pool needs
// web.proxy_pool, and search.paid_provider=allowed needs web.paid_search. The first
// missing entitlement, in that order, is named.
func (d WebPolicyDoc) CheckEntitled(ent WebEntitlements) error {
	if d.RobotsMax == webRobotsIgnore && !ent.RobotsOverride {
		return &WebPolicyError{Kind: ErrNotEntitled, Key: "robots.max_mode", Capability: WebRobotsOverrideEntitlementKey, Message: "ignoring robots.txt is not part of this plan"}
	}
	if d.ProxyMax == webProxyPool && !ent.ProxyPool {
		return &WebPolicyError{Kind: ErrNotEntitled, Key: "proxy.max_tier", Capability: WebProxyPoolEntitlementKey, Message: "the proxy pool is not part of this plan"}
	}
	if d.Caps[webCapPaidSearch] == WebModeAllowed && !ent.PaidSearch {
		return &WebPolicyError{Kind: ErrNotEntitled, Key: webCapPaidSearch, Capability: WebPaidSearchEntitlementKey, Message: "paid search providers are not part of this plan"}
	}
	return nil
}

// Effective folds the entitlements into the document, tightening only: a ceiling
// the plan does not unlock drops to the catalogue baseline, and paid search is
// disabled when the plan lacks it. A plan that gains an entitlement later makes the
// stored document effective again with no write. It is the document Auth Core
// mints, so a lapsed plan cannot keep a stored unlock.
func (d WebPolicyDoc) Effective(ent WebEntitlements) WebPolicyDoc {
	out := d.Normalize()
	if !ent.RobotsOverride && out.RobotsMax == webRobotsIgnore {
		out.RobotsMax = webRobotsRespect
	}
	if !ent.ProxyPool && out.ProxyMax == webProxyPool {
		out.ProxyMax = webProxyNone
	}
	if !ent.PaidSearch {
		if out.Caps == nil {
			out.Caps = map[string]string{}
		}
		out.Caps[webCapPaidSearch] = WebModeDisabled
	}
	return out.Normalize()
}

// webPolicyRank is the restrictiveness of a mode for a kind; a larger rank is
// more restrictive. The same table as quarry-core web_policy::rank.
func webPolicyRank(kind, mode string) int {
	if kind == "protection" {
		switch mode {
		case WebModeForced:
			return 2
		case WebModeAllowed:
			return 1
		default:
			return 0
		}
	}
	if mode == WebModeDisabled {
		return 1
	}
	return 0
}

func webRobotsRank(value string) int {
	switch value {
	case webRobotsStrict:
		return 0
	case webRobotsRespect:
		return 1
	default: // ignore, or nothing stated: the org layer adds no limit
		return 2
	}
}

func webProxyRank(value string) int {
	if value == webProxyNone {
		return 0
	}
	return 1
}

// Tightened reports whether replacing oldDoc with newDoc only tightens the policy:
// no dimension is looser and at least one is stricter. Equal documents, a loosening
// and a mixed change are not tightened. Dimensions: each capability's restrictiveness
// (an absent key is the neutral value of its kind), the robots and proxy ceilings
// (an absent ceiling states no limit), the allow list (absent or empty is
// unrestricted; a smaller set is stricter) and the deny list (a larger set is
// stricter).
func Tightened(oldDoc, newDoc WebPolicyDoc) bool {
	catalogue := DefaultWebPolicyCatalogue()
	o, n := oldDoc.Normalize(), newDoc.Normalize()
	tighter := false

	keys := map[string]struct{}{}
	for key := range o.Caps {
		keys[key] = struct{}{}
	}
	for key := range n.Caps {
		keys[key] = struct{}{}
	}
	for key := range keys {
		kind := "ability"
		if spec, ok := catalogue.Spec(key); ok {
			kind = spec.Kind
		}
		before, after := webPolicyRank(kind, o.Caps[key]), webPolicyRank(kind, n.Caps[key])
		if after < before {
			return false
		}
		if after > before {
			tighter = true
		}
	}

	switch before, after := webRobotsRank(o.RobotsMax), webRobotsRank(n.RobotsMax); {
	case after > before:
		return false
	case after < before:
		tighter = true
	}
	switch before, after := webProxyRank(o.ProxyMax), webProxyRank(n.ProxyMax); {
	case after > before:
		return false
	case after < before:
		tighter = true
	}

	oldAllow, newAllow := o.allow(), n.allow()
	switch {
	case len(oldAllow) == 0 && len(newAllow) > 0:
		tighter = true
	case len(oldAllow) > 0 && len(newAllow) == 0:
		return false
	case len(oldAllow) > 0:
		if !isSubset(newAllow, oldAllow) {
			return false
		}
		if len(newAllow) < len(oldAllow) {
			tighter = true
		}
	}

	oldDeny, newDeny := o.deny(), n.deny()
	if !isSubset(oldDeny, newDeny) {
		return false
	}
	if len(newDeny) > len(oldDeny) {
		tighter = true
	}
	return tighter
}

// isSubset reports whether every element of sub is in super.
func isSubset(sub, super []string) bool {
	set := make(map[string]struct{}, len(super))
	for _, item := range super {
		set[item] = struct{}{}
	}
	for _, item := range sub {
		if _, ok := set[item]; !ok {
			return false
		}
	}
	return true
}

// ChangedKeys names what differs between two documents, as catalogue keys:
// capability keys, robots.max_mode, proxy.max_tier, domains.allow and domains.deny.
// It is what the audit event carries: keys only, never a host name or a value.
func ChangedKeys(oldDoc, newDoc WebPolicyDoc) []string {
	o, n := oldDoc.Normalize(), newDoc.Normalize()
	changed := map[string]struct{}{}
	for key, mode := range n.Caps {
		if o.Caps[key] != mode {
			changed[key] = struct{}{}
		}
	}
	for key, mode := range o.Caps {
		if n.Caps[key] != mode {
			changed[key] = struct{}{}
		}
	}
	if o.RobotsMax != n.RobotsMax {
		changed["robots.max_mode"] = struct{}{}
	}
	if o.ProxyMax != n.ProxyMax {
		changed["proxy.max_tier"] = struct{}{}
	}
	if !equalStrings(o.allow(), n.allow()) {
		changed["domains.allow"] = struct{}{}
	}
	if !equalStrings(o.deny(), n.deny()) {
		changed["domains.deny"] = struct{}{}
	}
	keys := make([]string, 0, len(changed))
	for key := range changed {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// WebPolicyView is what a read returns: the stored document, the document folded
// with the plan's entitlements, and the revision both belong to.
type WebPolicyView struct {
	// Exists is false when the organization never wrote a policy (revision 0).
	Exists       bool
	Policy       WebPolicyDoc
	Effective    WebPolicyDoc
	Revision     int64
	Rev          string
	Entitlements map[string]bool
	UpdatedBy    *string
	UpdatedAt    *string
}

// WebPolicyPutResult is the outcome of a write.
type WebPolicyPutResult struct {
	Revision  int64
	Rev       string
	Tightened bool
}

// WebPolicyHistoryEntry is one append-only history row.
type WebPolicyHistoryEntry struct {
	Revision  int64        `json:"revision"`
	Rev       string       `json:"rev"`
	Policy    WebPolicyDoc `json:"policy"`
	ChangedBy string       `json:"changed_by"`
	ChangedAt string       `json:"changed_at"`
	Tightened bool         `json:"tightened"`
}

// webPolicyAbsentRev is the rev of an organization with no stored policy.
const webPolicyAbsentRev = "0"
