package zdr

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"reflect"
	"regexp"
	"strings"
)

// The capability contract is authored data that three planes read: integration-corev2
// (this package, go:embed), Auth Core (compares the opaque digest only) and inference-core
// (a Rust test instantiates the real provider capabilities and checks every row). The
// canonical copy lives in apps/Model Plane/api/contracts/. This loader is the Go half of
// the strict-loader rule written down in that directory's README: no unknown field, no
// missing field, no null where a value is required, every enum closed, and every digest
// vector and case recomputed by THIS package's own code so the golden never depends on one
// language's serialiser.

const (
	contractID      = "connection.zdr-capabilities"
	contractVersion = 1
)

var (
	statesList          = []string{string(CapabilitySupported), string(CapabilityNotSupported), string(CapabilityCannotVerify)}
	basesList           = []string{string(BasisVerevonAttestedDeployment), string(BasisProviderTermsKnown), string(BasisProviderByContract), string(BasisCustomerAsserted), string(BasisNotAssessed)}
	localesList         = []string{"no", "en"}
	wordingVariantsList = []string{"org_zdr_on", "org_zdr_off", "first_use"}
	digestFieldsList    = []string{"auth_mode", "basis", "capability", "catalog_version", "provider_key"}
	requiredCaseIDs     = []string{"unknown_provider", "known_provider_other_auth_mode", "empty_provider", "provider_alias_is_not_a_key"}

	snakePattern    = regexp.MustCompile(`^[a-z][a-z0-9_]*$`)
	basisRefPattern = regexp.MustCompile(`^[a-z0-9][a-z0-9-]*@[0-9]+$`)
	fallbackRef     = regexp.MustCompile(`^fallback@[0-9]+$`)
	sha256Pattern   = regexp.MustCompile(`^[0-9a-f]{64}$`)
)

type contractDoc struct {
	Contract        string          `json:"contract"`
	Version         int             `json:"version"`
	CatalogVersion  int             `json:"catalog_version"`
	Description     string          `json:"description"`
	States          []string        `json:"states"`
	Bases           []string        `json:"bases"`
	RunRule         runRuleDoc      `json:"run_rule"`
	Locales         []string        `json:"locales"`
	WordingVariants []string        `json:"wording_variants"`
	WordingKeys     []wordingKeyDoc `json:"wording_keys"`
	Digest          digestDoc       `json:"digest"`
	Providers       []providerDoc   `json:"providers"`
	Fallback        fallbackDoc     `json:"fallback"`
	Cases           []caseDoc       `json:"cases"`
}

type runRuleDoc struct {
	ZDRStaysOn              []string `json:"zdr_stays_on"`
	AcknowledgementRequired []string `json:"acknowledgement_required"`
}

type wordingKeyDoc struct {
	Key      string   `json:"key"`
	State    string   `json:"state"`
	Variants []string `json:"variants"`
}

type digestDoc struct {
	Algorithm     string      `json:"algorithm"`
	Encoding      string      `json:"encoding"`
	Fields        []string    `json:"fields"`
	CanonicalForm string      `json:"canonical_form"`
	Vectors       []vectorDoc `json:"vectors"`
}

type vectorDoc struct {
	ID        string         `json:"id"`
	Note      string         `json:"note"`
	Input     vectorInputDoc `json:"input"`
	Canonical string         `json:"canonical"`
	SHA256    string         `json:"sha256"`
}

type vectorInputDoc struct {
	ProviderKey    string `json:"provider_key"`
	AuthMode       string `json:"auth_mode"`
	Capability     string `json:"capability"`
	Basis          string `json:"basis"`
	CatalogVersion int    `json:"catalog_version"`
}

type providerDoc struct {
	BasisRef             string  `json:"basis_ref"`
	ProviderKey          string  `json:"provider_key"`
	AuthMode             string  `json:"auth_mode"`
	Capability           string  `json:"capability"`
	Basis                string  `json:"basis"`
	WaiverRoute          bool    `json:"waiver_route"`
	WordingKey           string  `json:"wording_key"`
	InferenceProviderID  *string `json:"inference_provider_id"`
	InferenceSupportsZDR *bool   `json:"inference_supports_zdr"`
	Evidence             string  `json:"evidence"`
}

type fallbackDoc struct {
	BasisRef    string `json:"basis_ref"`
	Capability  string `json:"capability"`
	Basis       string `json:"basis"`
	WaiverRoute bool   `json:"waiver_route"`
	WordingKey  string `json:"wording_key"`
}

type caseDoc struct {
	ID     string       `json:"id"`
	Input  caseInputDoc `json:"input"`
	Expect caseExpect   `json:"expect"`
}

type caseInputDoc struct {
	ProviderKey string `json:"provider_key"`
	AuthMode    string `json:"auth_mode"`
}

type caseExpect struct {
	BasisRef    string `json:"basis_ref"`
	Capability  string `json:"capability"`
	Basis       string `json:"basis"`
	WaiverRoute bool   `json:"waiver_route"`
	WordingKey  string `json:"wording_key"`
}

// ContractError lists every finding of a rejected contract, so one run shows all of them.
type ContractError struct {
	Findings []string
}

func (e *ContractError) Error() string {
	return "connection ZDR capability contract is invalid: " + strings.Join(e.Findings, "; ")
}

// parseContract decodes raw strictly and returns the document only when it passes every
// rule. It never returns a partly valid document.
func parseContract(raw []byte) (*contractDoc, error) {
	var findings []string
	if err := checkSingleDocument(raw); err != nil {
		return nil, &ContractError{Findings: []string{err.Error()}}
	}
	checkShape(json.RawMessage(raw), reflect.TypeOf(contractDoc{}), "$", &findings)
	if len(findings) > 0 {
		return nil, &ContractError{Findings: findings}
	}
	var doc contractDoc
	if err := json.Unmarshal(raw, &doc); err != nil {
		return nil, &ContractError{Findings: []string{"decode: " + err.Error()}}
	}
	if findings = validateDoc(&doc); len(findings) > 0 {
		return nil, &ContractError{Findings: findings}
	}
	return &doc, nil
}

func checkSingleDocument(raw []byte) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	var first json.RawMessage
	if err := decoder.Decode(&first); err != nil {
		return fmt.Errorf("not a JSON document: %w", err)
	}
	var extra json.RawMessage
	if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) {
		return errors.New("trailing data after the JSON document")
	}
	return nil
}

// checkShape is the Go form of "exact keys": every object has exactly the fields its Go type
// names, no field is null unless its Go type is a pointer, and arrays hold well-shaped
// elements. A missing field is a finding, never a zero value; an unknown field is a finding,
// never ignored.
func checkShape(raw json.RawMessage, t reflect.Type, path string, findings *[]string) {
	trimmed := bytes.TrimSpace(raw)
	isNull := bytes.Equal(trimmed, []byte("null"))
	switch t.Kind() {
	case reflect.Ptr:
		if isNull {
			return
		}
		checkShape(raw, t.Elem(), path, findings)
	case reflect.Struct:
		if isNull || len(trimmed) == 0 || trimmed[0] != '{' {
			*findings = append(*findings, path+": must be an object")
			return
		}
		var object map[string]json.RawMessage
		if err := json.Unmarshal(raw, &object); err != nil {
			*findings = append(*findings, path+": must be an object")
			return
		}
		known := map[string]reflect.StructField{}
		for i := 0; i < t.NumField(); i++ {
			field := t.Field(i)
			name := strings.Split(field.Tag.Get("json"), ",")[0]
			known[name] = field
		}
		for key := range object {
			if _, ok := known[key]; !ok {
				*findings = append(*findings, fmt.Sprintf("%s: unknown field %q", path, key))
			}
		}
		for name, field := range known {
			value, present := object[name]
			if !present {
				*findings = append(*findings, fmt.Sprintf("%s: missing field %q", path, name))
				continue
			}
			checkShape(value, field.Type, path+"."+name, findings)
		}
	case reflect.Slice:
		if isNull || len(trimmed) == 0 || trimmed[0] != '[' {
			*findings = append(*findings, path+": must be a list")
			return
		}
		var items []json.RawMessage
		if err := json.Unmarshal(raw, &items); err != nil {
			*findings = append(*findings, path+": must be a list")
			return
		}
		for index, item := range items {
			checkShape(item, t.Elem(), fmt.Sprintf("%s[%d]", path, index), findings)
		}
	default:
		if isNull {
			*findings = append(*findings, path+": must not be null")
		}
	}
}

func validateDoc(doc *contractDoc) []string {
	var out []string
	add := func(format string, args ...any) { out = append(out, fmt.Sprintf(format, args...)) }

	if doc.Contract != contractID {
		add("contract: must be %q", contractID)
	}
	if doc.Version != contractVersion {
		add("version: must be %d (a format change bumps it)", contractVersion)
	}
	if doc.CatalogVersion < 1 {
		add("catalog_version: must be a positive integer")
	}
	if strings.TrimSpace(doc.Description) == "" {
		add("description: must be a non-empty string")
	}
	if !sameList(doc.States, statesList) {
		add("states: must be exactly %s", strings.Join(statesList, ", "))
	}
	if !sameList(doc.Bases, basesList) {
		add("bases: must be exactly %s", strings.Join(basesList, ", "))
	}
	if !sameList(doc.Locales, localesList) {
		add("locales: must be exactly %s", strings.Join(localesList, ", "))
	}
	if !sameList(doc.WordingVariants, wordingVariantsList) {
		add("wording_variants: must be exactly %s", strings.Join(wordingVariantsList, ", "))
	}

	// run_rule: every state is either "ZDR stays on" or "needs an acknowledgement", exactly
	// once, and only supported keeps ZDR on. cannot_verify is never rounded up.
	covered := append(append([]string{}, doc.RunRule.ZDRStaysOn...), doc.RunRule.AcknowledgementRequired...)
	if !sameSet(covered, statesList) {
		add("run_rule: zdr_stays_on and acknowledgement_required must cover every state exactly once")
	} else if !sameList(doc.RunRule.ZDRStaysOn, []string{string(CapabilitySupported)}) {
		add("run_rule.zdr_stays_on: only supported keeps ZDR on; cannot_verify is never rounded up")
	}

	wording := map[string]wordingKeyDoc{}
	for _, w := range doc.WordingKeys {
		path := "wording_keys[" + w.Key + "]"
		if !snakePattern.MatchString(w.Key) {
			add("%s.key: must be snake_case", path)
		}
		if _, dup := wording[w.Key]; dup {
			add("%s: duplicate wording key", path)
		}
		wording[w.Key] = w
		if !contains(statesList, w.State) {
			add("%s.state: %q is not one of %s", path, w.State, strings.Join(statesList, ", "))
			continue
		}
		if !subsetUnique(w.Variants, wordingVariantsList) {
			add("%s.variants: must be a list of %s without repeats", path, strings.Join(wordingVariantsList, ", "))
			continue
		}
		needsAck := contains(doc.RunRule.AcknowledgementRequired, w.State)
		hasAll := contains(w.Variants, "org_zdr_on") && contains(w.Variants, "org_zdr_off") && contains(w.Variants, "first_use")
		if needsAck && !hasAll {
			add("%s.variants: a state that needs an acknowledgement needs org_zdr_on, org_zdr_off and first_use texts", path)
		}
		if !needsAck && (contains(w.Variants, "first_use") || !contains(w.Variants, "org_zdr_on") || !contains(w.Variants, "org_zdr_off")) {
			add("%s.variants: a state that needs no acknowledgement needs org_zdr_on and org_zdr_off texts and never first_use", path)
		}
	}
	for _, state := range statesList {
		found := false
		for _, w := range wording {
			if w.State == state {
				found = true
			}
		}
		if !found {
			add("wording_keys: no key for state %s", state)
		}
	}

	out = append(out, validateProviders(doc, wording)...)
	out = append(out, validateFallback(doc, wording)...)
	out = append(out, validateDigest(doc)...)
	out = append(out, validateCases(doc)...)
	return out
}

func validateProviders(doc *contractDoc, wording map[string]wordingKeyDoc) []string {
	var out []string
	add := func(format string, args ...any) { out = append(out, fmt.Sprintf(format, args...)) }
	if len(doc.Providers) == 0 {
		return []string{"providers: must be a non-empty list"}
	}
	refs := map[string]bool{}
	pairs := map[string]bool{}
	for _, p := range doc.Providers {
		path := "providers." + p.BasisRef
		if !basisRefPattern.MatchString(p.BasisRef) || strings.SplitN(p.BasisRef, "@", 2)[0] != p.ProviderKey {
			add("%s.basis_ref: must be \"<provider_key>@<row revision>\", a versioned row id", path)
		}
		if refs[p.BasisRef] {
			add("%s.basis_ref: duplicate basis_ref", path)
		}
		refs[p.BasisRef] = true
		if strings.TrimSpace(p.ProviderKey) == "" || strings.TrimSpace(p.AuthMode) == "" {
			add("%s: provider_key and auth_mode must be non-empty strings", path)
		}
		pair := p.ProviderKey + "\x00" + p.AuthMode
		if pairs[pair] {
			add("%s: provider_key and auth_mode pair repeats", path)
		}
		pairs[pair] = true
		if !contains(statesList, p.Capability) {
			add("%s.capability: %q is not one of %s", path, p.Capability, strings.Join(statesList, ", "))
		}
		if !contains(basesList, p.Basis) {
			add("%s.basis: %q is not one of %s", path, p.Basis, strings.Join(basesList, ", "))
		}
		if p.Capability == string(CapabilitySupported) && p.Basis != string(BasisVerevonAttestedDeployment) && p.Basis != string(BasisCustomerAsserted) {
			add("%s.basis: supported needs an attested deployment or a recorded customer assertion, never provider_terms_known or less", path)
		}
		if p.Basis == string(BasisNotAssessed) && p.Capability != string(CapabilityCannotVerify) {
			add("%s.basis: not_assessed can only mean cannot_verify", path)
		}
		if w, ok := wording[p.WordingKey]; !ok {
			add("%s.wording_key: %q is not a wording key", path, p.WordingKey)
		} else if w.State != p.Capability {
			add("%s.wording_key: %q is a %s text but the provider is %s (state mismatch)", path, p.WordingKey, w.State, p.Capability)
		}
		if (p.InferenceProviderID == nil) != (p.InferenceSupportsZDR == nil) {
			add("%s: inference_provider_id and inference_supports_zdr are both set or both null", path)
		} else if p.InferenceProviderID != nil {
			if strings.TrimSpace(*p.InferenceProviderID) == "" {
				add("%s: inference_provider_id must be a non-empty string", path)
			} else if (p.Capability == string(CapabilitySupported)) != *p.InferenceSupportsZDR {
				add("%s: capability %s disagrees with inference-core supports_zdr=%t; a connection is supported exactly when its provider reports supports_zdr", path, p.Capability, *p.InferenceSupportsZDR)
			}
		}
		if strings.TrimSpace(p.Evidence) == "" {
			add("%s.evidence: must say what the assessment rests on", path)
		}
	}
	return out
}

func validateFallback(doc *contractDoc, wording map[string]wordingKeyDoc) []string {
	var out []string
	f := doc.Fallback
	if !fallbackRef.MatchString(f.BasisRef) {
		out = append(out, "fallback.basis_ref: must be \"fallback@<row revision>\"")
	}
	if f.Capability != string(CapabilityCannotVerify) {
		out = append(out, "fallback.capability: an unassessed connection is cannot_verify, never supported or not_supported")
	}
	if f.Basis != string(BasisNotAssessed) {
		out = append(out, "fallback.basis: must be not_assessed")
	}
	if f.WaiverRoute {
		out = append(out, "fallback.waiver_route: an unassessed provider has no waiver route")
	}
	if w, ok := wording[f.WordingKey]; !ok || w.State != string(CapabilityCannotVerify) {
		out = append(out, "fallback.wording_key: must be a cannot_verify wording key")
	}
	return out
}

func validateDigest(doc *contractDoc) []string {
	var out []string
	add := func(format string, args ...any) { out = append(out, fmt.Sprintf(format, args...)) }
	d := doc.Digest
	if d.Algorithm != "sha256" {
		add("digest.algorithm: must be sha256")
	}
	if d.Encoding != "lowercase_hex" {
		add("digest.encoding: must be lowercase_hex")
	}
	if !sameList(d.Fields, digestFieldsList) {
		add("digest.fields: must be exactly %s in ascending order", strings.Join(digestFieldsList, ", "))
	}
	if strings.TrimSpace(d.CanonicalForm) == "" {
		add("digest.canonical_form: must describe the canonical form")
	}
	if len(d.Vectors) < 3 {
		add("digest.vectors: at least three vectors (the current digest, a catalog_version bump, a capability change)")
		return out
	}
	seenIDs := map[string]bool{}
	seenHashes := map[string]bool{}
	covered := map[string]bool{}
	for _, v := range d.Vectors {
		path := "digest.vectors[" + v.ID + "]"
		if !snakePattern.MatchString(v.ID) || seenIDs[v.ID] {
			add("%s.id: must be unique snake_case", path)
		}
		seenIDs[v.ID] = true
		if strings.TrimSpace(v.Note) == "" {
			add("%s.note: must say what the vector shows", path)
		}
		canonical, err := canonicalDigestInput(v.Input.ProviderKey, v.Input.AuthMode, Capability(v.Input.Capability), Basis(v.Input.Basis), v.Input.CatalogVersion)
		if err != nil {
			add("%s.input: %v", path, err)
			continue
		}
		if v.Canonical != canonical {
			add("%s.canonical: %q does not equal the canonical form of its input %q", path, v.Canonical, canonical)
		}
		expected := sha256Hex(canonical)
		if !sha256Pattern.MatchString(v.SHA256) || v.SHA256 != expected {
			add("%s.sha256: %q does not match the sha256 of canonical (%s)", path, v.SHA256, expected)
		}
		if seenHashes[v.SHA256] {
			add("%s.sha256: two vectors share a digest, so one of them shows nothing", path)
		}
		seenHashes[v.SHA256] = true
		covered[canonical] = true
	}
	// A digest vector must exist for every provider row at the current catalog_version: a bump
	// without a new vector is caught here, not in production.
	for _, p := range doc.Providers {
		want, err := canonicalDigestInput(p.ProviderKey, p.AuthMode, Capability(p.Capability), Basis(p.Basis), doc.CatalogVersion)
		if err != nil || !covered[want] {
			add("digest.vectors: no vector covers %s at catalog_version %d", p.BasisRef, doc.CatalogVersion)
		}
	}
	return out
}

func validateCases(doc *contractDoc) []string {
	var out []string
	add := func(format string, args ...any) { out = append(out, fmt.Sprintf(format, args...)) }
	if len(doc.Cases) == 0 {
		return []string{"cases: must be a non-empty list"}
	}
	rows := newRowIndex(doc)
	ids := map[string]bool{}
	for _, c := range doc.Cases {
		path := "cases[" + c.ID + "]"
		if !snakePattern.MatchString(c.ID) || ids[c.ID] {
			add("%s.id: must be unique snake_case", path)
		}
		ids[c.ID] = true
		want := rows.lookup(c.Input.ProviderKey, c.Input.AuthMode)
		got := caseExpect{BasisRef: want.basisRef, Capability: string(want.capability), Basis: string(want.basis), WaiverRoute: want.waiverRoute, WordingKey: want.wordingKey}
		if got != c.Expect {
			add("%s.expect: must equal the exact-match-else-fallback assessment %+v; only an exact (provider_key, auth_mode) row is assessed, anything else gets the fallback", path, got)
		}
	}
	for _, id := range requiredCaseIDs {
		if !ids[id] {
			add("cases: required case %q is missing", id)
		}
	}
	for _, p := range doc.Providers {
		found := false
		for _, c := range doc.Cases {
			if c.Input.ProviderKey == p.ProviderKey && c.Input.AuthMode == p.AuthMode {
				found = true
			}
		}
		if !found {
			add("cases: no case assesses %s", p.BasisRef)
		}
	}
	return out
}

// row is one assessable outcome: a provider row of the contract, or the fallback.
type row struct {
	basisRef    string
	capability  Capability
	basis       Basis
	waiverRoute bool
	wordingKey  string
}

type rowIndex struct {
	byPair   map[string]row
	fallback row
}

func newRowIndex(doc *contractDoc) rowIndex {
	index := rowIndex{byPair: make(map[string]row, len(doc.Providers))}
	for _, p := range doc.Providers {
		index.byPair[p.ProviderKey+"\x00"+p.AuthMode] = row{
			basisRef: p.BasisRef, capability: Capability(p.Capability), basis: Basis(p.Basis),
			waiverRoute: p.WaiverRoute, wordingKey: p.WordingKey,
		}
	}
	f := doc.Fallback
	index.fallback = row{basisRef: f.BasisRef, capability: Capability(f.Capability), basis: Basis(f.Basis), waiverRoute: f.WaiverRoute, wordingKey: f.WordingKey}
	return index
}

// lookup is the whole assessment rule: an exact (provider_key, auth_mode) pair, else the
// fallback. A near match (an alias, another auth mode of a known provider, an empty key) is
// never assessed as the known provider.
func (i rowIndex) lookup(providerKey, authMode string) row {
	if found, ok := i.byPair[providerKey+"\x00"+authMode]; ok {
		return found
	}
	return i.fallback
}

func contains(list []string, value string) bool {
	for _, item := range list {
		if item == value {
			return true
		}
	}
	return false
}

func sameList(a, b []string) bool {
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

func sameSet(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	seen := map[string]bool{}
	for _, item := range a {
		if seen[item] || !contains(b, item) {
			return false
		}
		seen[item] = true
	}
	return true
}

func subsetUnique(values, allowed []string) bool {
	seen := map[string]bool{}
	for _, value := range values {
		if !contains(allowed, value) || seen[value] {
			return false
		}
		seen[value] = true
	}
	return true
}
