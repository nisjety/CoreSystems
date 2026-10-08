package zdr

import (
	"bytes"
	"encoding/json"
	"os"
	"strings"
	"testing"
)

// Digests below were computed by the JavaScript reference serialiser
// (scripts/tests/zdr-contract-schema.mjs digestOf) and are listed in the contract's own
// digest.vectors. They are an oracle independent of this package's serialiser.
const (
	digestCodexCatalog1       = "7d865ca0c6296627837c0603192debcd2c217bcb17b884738fe99befdb810fbd"
	digestCodexCatalog2       = "74776afca7304ff1484468369d18b2d29bac1c9fe803c1efa6a02eb04a5f50f3"
	digestUnassessedFallback  = "451731439b57be37fc4e84c10c1d324e16c7f8bd0d9ad80b62eb765387e8d0e3"
	digestCodexCapabilityFlip = "ae03bb4e049bb582c476c92e6b86b2c14159c8744083a622f5bb07e6a0b733e8"
)

const (
	codexProvider = "openai-codex-subscription"
	codexAuthMode = "codex_chatgpt_device_code"
)

func mustDefault(t *testing.T) *Assessor {
	t.Helper()
	assessor, err := Default()
	if err != nil {
		t.Fatalf("Default: %v", err)
	}
	return assessor
}

func TestDefaultLoadsTheEmbeddedContractAndItsPinMatchesTheBytes(t *testing.T) {
	assessor := mustDefault(t)
	raw, err := os.ReadFile("testdata/connection-zdr-capabilities.json")
	if err != nil {
		t.Fatalf("read vendored contract: %v", err)
	}
	pin, err := os.ReadFile("testdata/connection-zdr-capabilities.json.sha256")
	if err != nil {
		t.Fatalf("read vendored pin: %v", err)
	}
	if !bytes.Equal(raw, embeddedContract) || !bytes.Equal(pin, embeddedPin) {
		t.Fatal("the embedded bytes differ from the vendored files on disk")
	}
	want := sha256Hex(strings.ReplaceAll(string(raw), "\r\n", "\n"))
	if got := assessor.PinnedSHA256(); got != want || firstField(string(pin)) != want {
		t.Fatalf("pinned digest = %s, pin file = %s, actual = %s; all three must agree", got, firstField(string(pin)), want)
	}
	if assessor.CatalogVersion() < 1 {
		t.Fatalf("catalog_version = %d", assessor.CatalogVersion())
	}
}

func TestEveryDigestVectorOfTheContractIsReproducedByThisSerialiser(t *testing.T) {
	assessor := mustDefault(t)
	if len(assessor.doc.Digest.Vectors) < 4 {
		t.Fatalf("contract carries %d vectors, want the four the README names", len(assessor.doc.Digest.Vectors))
	}
	for _, vector := range assessor.doc.Digest.Vectors {
		in := vector.Input
		got, err := Digest(in.ProviderKey, in.AuthMode, Capability(in.Capability), Basis(in.Basis), in.CatalogVersion)
		if err != nil {
			t.Fatalf("vector %s: %v", vector.ID, err)
		}
		if got != vector.SHA256 {
			t.Errorf("vector %s: digest = %s, contract says %s", vector.ID, got, vector.SHA256)
		}
		canonical, err := canonicalDigestInput(in.ProviderKey, in.AuthMode, Capability(in.Capability), Basis(in.Basis), in.CatalogVersion)
		if err != nil || canonical != vector.Canonical {
			t.Errorf("vector %s: canonical = %q (err %v), contract says %q", vector.ID, canonical, err, vector.Canonical)
		}
	}
	// The four README vectors by literal digest, so a contract edit that moves them is a
	// deliberate act here too and not only in the JSON.
	cases := []struct {
		name       string
		provider   string
		authMode   string
		capability Capability
		basis      Basis
		version    int
		want       string
	}{
		{"codex catalog 1", codexProvider, codexAuthMode, CapabilityNotSupported, BasisProviderTermsKnown, 1, digestCodexCatalog1},
		{"codex catalog 2", codexProvider, codexAuthMode, CapabilityNotSupported, BasisProviderTermsKnown, 2, digestCodexCatalog2},
		{"unassessed fallback", "example-llm-gateway", "api_key", CapabilityCannotVerify, BasisNotAssessed, 1, digestUnassessedFallback},
		{"codex capability flip", codexProvider, codexAuthMode, CapabilitySupported, BasisVerevonAttestedDeployment, 1, digestCodexCapabilityFlip},
	}
	for _, tc := range cases {
		got, err := Digest(tc.provider, tc.authMode, tc.capability, tc.basis, tc.version)
		if err != nil || got != tc.want {
			t.Errorf("%s: digest = %q (err %v), want %s", tc.name, got, err, tc.want)
		}
	}
}

func TestEveryCaseOfTheContractIsReproducedByTheAssessor(t *testing.T) {
	assessor := mustDefault(t)
	if len(assessor.doc.Cases) < 5 {
		t.Fatalf("contract carries %d cases, want at least the five it ships", len(assessor.doc.Cases))
	}
	for _, c := range assessor.doc.Cases {
		got, err := assessor.Assess(c.Input.ProviderKey, c.Input.AuthMode)
		if err != nil {
			t.Fatalf("case %s: %v", c.ID, err)
		}
		gotExpect := caseExpect{BasisRef: got.BasisRef, Capability: string(got.Capability), Basis: string(got.Basis), WaiverRoute: got.WaiverRoute, WordingKey: got.WordingKey}
		if gotExpect != c.Expect {
			t.Errorf("case %s: assessed %+v, contract expects %+v", c.ID, gotExpect, c.Expect)
		}
	}
}

func TestTheChatGPTSubscriptionIsNotSupportedOnKnownTerms(t *testing.T) {
	got, err := mustDefault(t).Assess(codexProvider, codexAuthMode)
	if err != nil {
		t.Fatalf("Assess: %v", err)
	}
	if got.Capability != CapabilityNotSupported || got.Basis != BasisProviderTermsKnown || got.BasisRef != "openai-codex-subscription@1" {
		t.Fatalf("assessment = %+v", got)
	}
	if !got.WaiverRoute || got.WordingKey != "chatgpt_subscription_not_supported" {
		t.Fatalf("waiver route / wording = %v / %q", got.WaiverRoute, got.WordingKey)
	}
	if got.CatalogVersion != 1 || got.Digest != digestCodexCatalog1 {
		t.Fatalf("catalog version / digest = %d / %s, want 1 / %s", got.CatalogVersion, got.Digest, digestCodexCatalog1)
	}
	if !got.Capability.RequiresAcknowledgement() || got.Capability.KeepsZDR() {
		t.Fatal("not_supported must require an acknowledgement and never keep ZDR on")
	}
}

func TestAnUnassessedConnectionIsCannotVerifyAndHashedUnderItsOwnKeys(t *testing.T) {
	got, err := mustDefault(t).Assess("example-llm-gateway", "api_key")
	if err != nil {
		t.Fatalf("Assess: %v", err)
	}
	if got.Capability != CapabilityCannotVerify || got.Basis != BasisNotAssessed || got.BasisRef != "fallback@1" || got.WaiverRoute {
		t.Fatalf("fallback assessment = %+v", got)
	}
	if got.Digest != digestUnassessedFallback {
		t.Fatalf("fallback digest = %s, want %s (its own provider_key and auth_mode are in it)", got.Digest, digestUnassessedFallback)
	}
	other, err := mustDefault(t).Assess("another-gateway", "api_key")
	if err != nil {
		t.Fatalf("Assess: %v", err)
	}
	if other.Digest == got.Digest {
		t.Fatal("two different unassessed providers share a digest, so an acknowledgement of one would cover the other")
	}
	if got.Capability.KeepsZDR() || !got.Capability.RequiresAcknowledgement() {
		t.Fatal("cannot_verify must be treated like not_supported: never kept on, always acknowledged")
	}
}

func TestOnlyAnExactProviderAndAuthModePairIsAssessedAsThatProvider(t *testing.T) {
	assessor := mustDefault(t)
	near := []struct {
		name, provider, authMode string
	}{
		{"alias", "codex-subscription", codexAuthMode},
		{"other auth mode of a known provider", codexProvider, "api_key"},
		{"empty", "", ""},
		{"upper case", "OpenAI-Codex-Subscription", codexAuthMode},
		{"padded", " " + codexProvider, codexAuthMode},
		{"trailing newline", codexProvider + "\n", codexAuthMode},
		{"auth mode taken from the connection's own provider_context", codexProvider, "chatgpt_device_code"},
	}
	for _, tc := range near {
		got, err := assessor.Assess(tc.provider, tc.authMode)
		if err != nil {
			t.Fatalf("%s: %v", tc.name, err)
		}
		if got.Capability != CapabilityCannotVerify || got.BasisRef != "fallback@1" || got.WaiverRoute {
			t.Errorf("%s: assessed as %+v, want the fallback", tc.name, got)
		}
	}
}

func TestTheDigestChangesWithEveryFieldAnAcknowledgementIsBoundTo(t *testing.T) {
	base, err := Digest(codexProvider, codexAuthMode, CapabilityNotSupported, BasisProviderTermsKnown, 1)
	if err != nil {
		t.Fatal(err)
	}
	variants := map[string]func() (string, error){
		"provider_key": func() (string, error) {
			return Digest("other", codexAuthMode, CapabilityNotSupported, BasisProviderTermsKnown, 1)
		},
		"auth_mode": func() (string, error) {
			return Digest(codexProvider, "other", CapabilityNotSupported, BasisProviderTermsKnown, 1)
		},
		"capability": func() (string, error) {
			return Digest(codexProvider, codexAuthMode, CapabilityCannotVerify, BasisProviderTermsKnown, 1)
		},
		"basis": func() (string, error) {
			return Digest(codexProvider, codexAuthMode, CapabilityNotSupported, BasisProviderByContract, 1)
		},
		"catalog_version": func() (string, error) {
			return Digest(codexProvider, codexAuthMode, CapabilityNotSupported, BasisProviderTermsKnown, 2)
		},
	}
	for field, compute := range variants {
		got, err := compute()
		if err != nil {
			t.Fatalf("%s: %v", field, err)
		}
		if got == base {
			t.Errorf("changing %s does not change the digest, so an acknowledgement would survive it", field)
		}
	}
	again, _ := Digest(codexProvider, codexAuthMode, CapabilityNotSupported, BasisProviderTermsKnown, 1)
	if again != base {
		t.Fatal("the digest is not deterministic")
	}
	if !IsDigest(base) || IsDigest(strings.ToUpper(base)) || IsDigest(base[:63]) {
		t.Fatal("IsDigest accepts a malformed digest or rejects a good one")
	}
}

// The canonical strings and digests below come from scripts/tests/zdr-contract-schema.mjs
// (JSON.stringify), for the inputs that make a serialiser differ: quote, backslash, every
// kind of control character, and the characters Go's encoding/json would escape.
func TestTheCanonicalFormMatchesTheReferenceSerialiserForAwkwardStrings(t *testing.T) {
	cases := []struct {
		name       string
		provider   string
		authMode   string
		capability Capability
		basis      Basis
		version    int
		canonical  string
		digest     string
	}{
		{
			name:       "quote backslash and controls",
			provider:   "a\"b\\c",
			authMode:   "x\ny\tz\u0001\u001f\b\f\r",
			capability: CapabilityCannotVerify,
			basis:      BasisNotAssessed,
			version:    1,
			canonical:  "{\"auth_mode\":\"x\\ny\\tz\\u0001\\u001f\\b\\f\\r\",\"basis\":\"not_assessed\",\"capability\":\"cannot_verify\",\"catalog_version\":1,\"provider_key\":\"a\\\"b\\\\c\"}",
			digest:     "157a611c343b1728f16049d9be42e7f703c9188652b074665714e42ed3befb24",
		},
		{
			name:       "html characters, line separators, DEL, non-ASCII",
			provider:   "<>&  \u007f",
			authMode:   "é€\U0001F600",
			capability: CapabilityNotSupported,
			basis:      BasisProviderTermsKnown,
			version:    12,
			canonical:  "{\"auth_mode\":\"é€\U0001F600\",\"basis\":\"provider_terms_known\",\"capability\":\"not_supported\",\"catalog_version\":12,\"provider_key\":\"<>&  \u007f\"}",
			digest:     "ebf87984086a0c1a72dbc9b12d6b32f1c33c28a607f536ea2e7d856b84102352",
		},
		{
			name:       "empty",
			provider:   "",
			authMode:   "",
			capability: CapabilityCannotVerify,
			basis:      BasisNotAssessed,
			version:    1,
			canonical:  "{\"auth_mode\":\"\",\"basis\":\"not_assessed\",\"capability\":\"cannot_verify\",\"catalog_version\":1,\"provider_key\":\"\"}",
			digest:     "75479c7d7969145d472588cf3ee78090e14163f9d24fa11f12e481c4143fe8e1",
		},
	}
	for _, tc := range cases {
		canonical, err := canonicalDigestInput(tc.provider, tc.authMode, tc.capability, tc.basis, tc.version)
		if err != nil {
			t.Fatalf("%s: %v", tc.name, err)
		}
		if canonical != tc.canonical {
			t.Errorf("%s: canonical = %q, reference = %q", tc.name, canonical, tc.canonical)
		}
		got, err := Digest(tc.provider, tc.authMode, tc.capability, tc.basis, tc.version)
		if err != nil || got != tc.digest {
			t.Errorf("%s: digest = %q (err %v), reference = %s", tc.name, got, err, tc.digest)
		}
	}
}

func TestAStringThatIsNotUTF8HasNoDigestAndIsNeverGuessed(t *testing.T) {
	if _, err := Digest("bad\xff", codexAuthMode, CapabilityNotSupported, BasisProviderTermsKnown, 1); err == nil {
		t.Fatal("Digest accepted an invalid UTF-8 provider_key")
	}
	if _, err := mustDefault(t).Assess(codexProvider, "bad\xc3\x28"); err == nil {
		t.Fatal("Assess accepted an invalid UTF-8 auth_mode")
	}
}

func TestCapabilityRunRule(t *testing.T) {
	if !CapabilitySupported.KeepsZDR() || CapabilitySupported.RequiresAcknowledgement() {
		t.Error("supported keeps ZDR on and needs no acknowledgement")
	}
	for _, c := range []Capability{CapabilityNotSupported, CapabilityCannotVerify} {
		if c.KeepsZDR() || !c.RequiresAcknowledgement() {
			t.Errorf("%s must not keep ZDR on and must require an acknowledgement", c)
		}
	}
	if Capability("maybe").Valid() || Capability("maybe").RequiresAcknowledgement() || Capability("").Valid() {
		t.Error("an unknown capability is valid, or would require an acknowledgement as if it were a state")
	}
	if !BasisNotAssessed.Valid() || Basis("guess").Valid() {
		t.Error("Basis.Valid disagrees with the five bases")
	}
}

// ---- the strict loader -------------------------------------------------------------------

// mutated returns the embedded contract with edit applied, re-serialised, together with the
// pin of the NEW bytes, so a rejection can only come from the strict loader and never from
// the pin check.
func mutated(t *testing.T, edit func(doc map[string]any)) ([]byte, string) {
	t.Helper()
	var doc map[string]any
	if err := json.Unmarshal(embeddedContract, &doc); err != nil {
		t.Fatalf("decode embedded contract: %v", err)
	}
	edit(doc)
	raw, err := json.MarshalIndent(doc, "", "  ")
	if err != nil {
		t.Fatalf("encode mutated contract: %v", err)
	}
	raw = append(raw, '\n')
	return raw, sha256Hex(string(raw)) + "  connection-zdr-capabilities.json\n"
}

func providerRow(doc map[string]any, index int) map[string]any {
	return doc["providers"].([]any)[index].(map[string]any)
}

func TestTheUnmodifiedContractPassesTheMutationHarness(t *testing.T) {
	raw, pin := mutated(t, func(map[string]any) {})
	if _, err := Load(raw, pin); err != nil {
		t.Fatalf("the harness itself breaks the contract, so every rejection below proves nothing: %v", err)
	}
}

func TestTheStrictLoaderRejectsEveryMalformedContract(t *testing.T) {
	cases := []struct {
		name string
		edit func(doc map[string]any)
		want string
	}{
		{"unknown top-level field", func(d map[string]any) { d["surprise"] = true }, `unknown field "surprise"`},
		{"missing top-level field", func(d map[string]any) { delete(d, "fallback") }, `missing field "fallback"`},
		{"null where a value is required", func(d map[string]any) { d["description"] = nil }, "must not be null"},
		{"unknown field in a provider row", func(d map[string]any) { providerRow(d, 0)["extra"] = 1 }, `unknown field "extra"`},
		{"missing field in a provider row", func(d map[string]any) { delete(providerRow(d, 0), "evidence") }, `missing field "evidence"`},
		{"missing field in a case", func(d map[string]any) {
			delete(d["cases"].([]any)[0].(map[string]any)["expect"].(map[string]any), "wording_key")
		}, `missing field "wording_key"`},
		{"unknown field in the digest block", func(d map[string]any) { d["digest"].(map[string]any)["salt"] = "x" }, `unknown field "salt"`},
		{"wrong contract id", func(d map[string]any) { d["contract"] = "something.else" }, "contract: must be"},
		{"format version bumped", func(d map[string]any) { d["version"] = 2 }, "version: must be 1"},
		{"catalog_version zero", func(d map[string]any) { d["catalog_version"] = 0 }, "catalog_version: must be a positive integer"},
		{"states reordered", func(d map[string]any) { d["states"] = []any{"not_supported", "supported", "cannot_verify"} }, "states: must be exactly"},
		{"a fourth state", func(d map[string]any) { d["states"] = []any{"supported", "not_supported", "cannot_verify", "mostly"} }, "states: must be exactly"},
		{"cannot_verify rounded up to keep ZDR on", func(d map[string]any) {
			d["run_rule"] = map[string]any{"zdr_stays_on": []any{"supported", "cannot_verify"}, "acknowledgement_required": []any{"not_supported"}}
		}, "only supported keeps ZDR on"},
		{"a state in neither run rule list", func(d map[string]any) {
			d["run_rule"] = map[string]any{"zdr_stays_on": []any{"supported"}, "acknowledgement_required": []any{"not_supported"}}
		}, "cover every state exactly once"},
		{"provider capability outside the three states", func(d map[string]any) { providerRow(d, 0)["capability"] = "mostly_supported" }, "is not one of supported, not_supported, cannot_verify"},
		{"provider basis outside the five", func(d map[string]any) { providerRow(d, 0)["basis"] = "vibes" }, "providers."},
		{"supported on provider terms alone", func(d map[string]any) {
			row := providerRow(d, 0)
			row["capability"] = "supported"
			row["inference_supports_zdr"] = true
			row["wording_key"] = "connection_supported"
		}, "supported needs an attested deployment or a recorded customer assertion"},
		{"supported but the inference provider says no ZDR", func(d map[string]any) {
			row := providerRow(d, 0)
			row["capability"] = "supported"
			row["basis"] = "verevon_attested_deployment"
			row["wording_key"] = "connection_supported"
		}, "disagrees with inference-core supports_zdr"},
		{"not_assessed that claims a capability", func(d map[string]any) { providerRow(d, 0)["basis"] = "not_assessed" }, "not_assessed can only mean cannot_verify"},
		{"inference id without a ZDR flag", func(d map[string]any) { providerRow(d, 0)["inference_supports_zdr"] = nil }, "both set or both null"},
		{"wording key of another state", func(d map[string]any) { providerRow(d, 0)["wording_key"] = "connection_supported" }, "state mismatch"},
		{"unknown wording key", func(d map[string]any) { providerRow(d, 0)["wording_key"] = "no_such_text" }, "is not a wording key"},
		{"basis_ref of another provider", func(d map[string]any) { providerRow(d, 0)["basis_ref"] = "someone-else@1" }, "basis_ref: must be"},
		{"the same provider and auth mode twice", func(d map[string]any) {
			rows := d["providers"].([]any)
			clone := map[string]any{}
			for k, v := range rows[0].(map[string]any) {
				clone[k] = v
			}
			clone["basis_ref"] = "openai-codex-subscription@2"
			d["providers"] = append(rows, clone)
		}, "pair repeats"},
		{"fallback that claims support", func(d map[string]any) { d["fallback"].(map[string]any)["capability"] = "supported" }, "fallback.capability"},
		{"fallback with a waiver route", func(d map[string]any) { d["fallback"].(map[string]any)["waiver_route"] = true }, "fallback.waiver_route"},
		{"fallback basis assessed", func(d map[string]any) { d["fallback"].(map[string]any)["basis"] = "provider_terms_known" }, "fallback.basis"},
		{"digest algorithm changed", func(d map[string]any) { d["digest"].(map[string]any)["algorithm"] = "sha512" }, "digest.algorithm"},
		{"digest fields changed", func(d map[string]any) {
			d["digest"].(map[string]any)["fields"] = []any{"auth_mode", "capability", "provider_key"}
		}, "digest.fields"},
		{"a digest vector with the wrong sha256", func(d map[string]any) {
			d["digest"].(map[string]any)["vectors"].([]any)[0].(map[string]any)["sha256"] = strings.Repeat("0", 64)
		}, "does not match the sha256"},
		{"a digest vector with the wrong canonical form", func(d map[string]any) {
			d["digest"].(map[string]any)["vectors"].([]any)[0].(map[string]any)["canonical"] = "{}"
		}, "does not equal the canonical form"},
		{"too few digest vectors", func(d map[string]any) {
			vectors := d["digest"].(map[string]any)["vectors"].([]any)
			d["digest"].(map[string]any)["vectors"] = vectors[:2]
		}, "at least three vectors"},
		{"catalog_version bumped without a vector", func(d map[string]any) { d["catalog_version"] = 3 }, "no vector covers"},
		{"a case that expects the wrong capability", func(d map[string]any) {
			d["cases"].([]any)[1].(map[string]any)["expect"].(map[string]any)["capability"] = "not_supported"
		}, "exact-match-else-fallback"},
		{"a required case removed", func(d map[string]any) {
			var kept []any
			for _, c := range d["cases"].([]any) {
				if c.(map[string]any)["id"] != "provider_alias_is_not_a_key" {
					kept = append(kept, c)
				}
			}
			d["cases"] = kept
		}, `required case "provider_alias_is_not_a_key" is missing`},
		{"an acknowledgement state without a first_use text", func(d map[string]any) {
			d["wording_keys"].([]any)[0].(map[string]any)["variants"] = []any{"org_zdr_on", "org_zdr_off"}
		}, "needs org_zdr_on, org_zdr_off and first_use texts"},
		{"a supported text offered at first use", func(d map[string]any) {
			d["wording_keys"].([]any)[2].(map[string]any)["variants"] = []any{"org_zdr_on", "org_zdr_off", "first_use"}
		}, "needs no acknowledgement"},
		{"no providers", func(d map[string]any) { d["providers"] = []any{} }, "providers: must be a non-empty list"},
		{"an empty locale list", func(d map[string]any) { d["locales"] = []any{} }, "locales: must be exactly"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			raw, pin := mutated(t, tc.edit)
			assessor, err := Load(raw, pin)
			if err == nil {
				t.Fatalf("Load accepted a contract with %s (assessor %v)", tc.name, assessor != nil)
			}
			if !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("Load error = %v, want it to mention %q", err, tc.want)
			}
		})
	}
}

func TestTheLoaderRefusesWhatIsNotExactlyOneJSONDocument(t *testing.T) {
	for name, raw := range map[string][]byte{
		"not json":          []byte("not json"),
		"empty":             {},
		"an array":          []byte("[]"),
		"trailing document": append(append([]byte{}, embeddedContract...), []byte("{}")...),
		"trailing garbage":  append(append([]byte{}, embeddedContract...), []byte("x")...),
		"truncated":         embeddedContract[:len(embeddedContract)/2],
		"null":              []byte("null"),
		"a string":          []byte(`"connection.zdr-capabilities"`),
		"an empty object":   []byte("{}"),
	} {
		pin := sha256Hex(string(raw)) + "  connection-zdr-capabilities.json\n"
		if _, err := Load(raw, pin); err == nil {
			t.Errorf("%s: Load accepted it", name)
		}
	}
}

func TestTheLoaderRefusesBytesThatDifferFromThePinnedCopy(t *testing.T) {
	good := string(embeddedPin)
	altered := bytes.Replace(embeddedContract, []byte("not_supported"), []byte("supported"), 1)
	if bytes.Equal(altered, embeddedContract) {
		t.Fatal("the test edit changed nothing")
	}
	if _, err := Load(altered, good); err == nil || !strings.Contains(err.Error(), "pinned digest") {
		t.Fatalf("Load accepted bytes that differ from the pin: %v", err)
	}
	for name, pin := range map[string]string{
		"empty pin":      "",
		"short pin":      "abc  connection-zdr-capabilities.json\n",
		"upper-case pin": strings.ToUpper(firstField(good))[:10] + "  x",
		"non hex pin":    strings.Repeat("z", 64) + "  x",
	} {
		if _, err := Load(embeddedContract, pin); err == nil {
			t.Errorf("%s: Load accepted it", name)
		}
	}
	// The pin is the hash of the file with CRLF normalised to LF (a Windows checkout may
	// rewrite line endings), so the same contract with CRLF endings still loads.
	crlf := bytes.ReplaceAll(embeddedContract, []byte("\n"), []byte("\r\n"))
	if _, err := Load(crlf, good); err != nil {
		t.Fatalf("a CRLF copy of the pinned contract was refused: %v", err)
	}
	// An upper-case spelling of the right digest is the same digest.
	if _, err := Load(embeddedContract, strings.ToUpper(firstField(good))+"  x\n"); err != nil {
		t.Fatalf("the pin comparison is case sensitive: %v", err)
	}
}

func TestProvidersAndWordingKeysAreReadOnlyViews(t *testing.T) {
	assessor := mustDefault(t)
	rows := assessor.Providers()
	if len(rows) == 0 {
		t.Fatal("no provider rows")
	}
	rows[0].Capability = CapabilitySupported
	if mustDefault(t).Providers()[0].Capability == CapabilitySupported {
		t.Fatal("Providers() hands out the assessor's own storage")
	}
	keys := assessor.WordingKeys()
	if keys["chatgpt_subscription_not_supported"] != CapabilityNotSupported || keys["connection_cannot_verify"] != CapabilityCannotVerify || keys["connection_supported"] != CapabilitySupported {
		t.Fatalf("wording keys = %v", keys)
	}
}
