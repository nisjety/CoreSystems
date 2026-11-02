package browsersites

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"reflect"
	"testing"
	"time"
)

// goldenDomainCanonical is the broker's host normaliser, vendored from the Model
// Plane (browser-domain-canonical.json). A host the golden refuses must be refused
// here, and one it accepts must come back in the golden's spelling.
type goldenDomainCanonical struct {
	Vectors []struct {
		ID     string   `json:"id"`
		Input  []string `json:"input"`
		Expect string   `json:"expect"`
		Output []string `json:"output"`
	} `json:"vectors"`
}

func TestCanonicalHostListMatchesTheBrokerGolden(t *testing.T) {
	raw, err := os.ReadFile("testdata/browser-domain-canonical.json")
	if err != nil {
		t.Fatalf("read vendored golden: %v", err)
	}
	var golden goldenDomainCanonical
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	if len(golden.Vectors) == 0 {
		t.Fatal("golden has no vectors")
	}
	for _, vector := range golden.Vectors {
		got, err := CanonicalHostList(vector.Input)
		switch vector.Expect {
		case "accept":
			if err != nil {
				t.Errorf("%s: accepted by the golden, refused here: %v", vector.ID, err)
				continue
			}
			if !reflect.DeepEqual(got, vector.Output) {
				t.Errorf("%s: got %q, golden says %q", vector.ID, got, vector.Output)
			}
		case "reject":
			if err == nil {
				t.Errorf("%s: refused by the golden, accepted here as %q", vector.ID, got)
			}
		default:
			t.Fatalf("%s: unknown expectation %q", vector.ID, vector.Expect)
		}
	}
}

func TestCanonicalHostRefusesAddressesAndDecorations(t *testing.T) {
	for _, host := range []string{"", "localhost", "10.0.0.1", "example.com.", "https://example.com", "example.com:8443", "ex*ample.com"} {
		if _, err := CanonicalHost(host); err == nil {
			t.Errorf("%q must be refused", host)
		}
	}
	if got, err := CanonicalHost("  Shop.Example.COM "); err != nil || got != "shop.example.com" {
		t.Errorf("a padded mixed-case host must fold to its canonical form, got %q err=%v", got, err)
	}
}

func TestValidatePutNormalisesAndNamesEveryProblem(t *testing.T) {
	revision := int64(3)
	normalised, err := ValidatePut(PutRequest{
		ExpectedRevision: &revision,
		Mode:             " allowlist ",
		Rules: []Rule{
			{Host: "zeta.example.com", Rule: "act", Note: "  checkout  "},
			{Host: "Alpha.Example.com", Rule: "read"},
		},
	})
	if err != nil {
		t.Fatalf("a valid write must pass: %v", err)
	}
	if normalised.Mode != ModeAllowlist || *normalised.ExpectedRevision != 3 {
		t.Fatalf("mode and revision must be normalised: %+v", normalised)
	}
	if normalised.Rules[0].Host != "alpha.example.com" || normalised.Rules[1].Note != "checkout" {
		t.Fatalf("rules must be canonical and sorted with notes trimmed: %+v", normalised.Rules)
	}

	_, err = ValidatePut(PutRequest{
		Mode: "wide",
		Rules: []Rule{
			{Host: "example.com", Rule: "block"},
			{Host: "EXAMPLE.com", Rule: "read"},
			{Host: "shop.example.com", Rule: "allow"},
			{Host: "*.example.org", Rule: "act"},
		},
	})
	var invalid *ValidationError
	if !errors.As(err, &invalid) {
		t.Fatalf("an invalid write must be a ValidationError, got %v", err)
	}
	want := map[string]bool{
		"expected_revision:" + CodeExpectedRevisionRequired: true,
		"mode:" + CodeInvalidMode:                           true,
		"rules[1].host:" + CodeDuplicateHost:                true,
		"rules[2].rule:" + CodeInvalidRule:                  true,
		"rules[3].host:" + CodeInvalidDomainPolicy:          true,
	}
	for _, field := range invalid.Errors {
		delete(want, field.Path+":"+field.Code)
	}
	if len(want) != 0 {
		t.Fatalf("missing field errors %v in %+v", want, invalid.Errors)
	}
}

func TestValidatePutRefusesTooManyRulesAndLongNotes(t *testing.T) {
	revision := int64(0)
	rules := make([]Rule, 0, MaxRules+1)
	for i := 0; i <= MaxRules; i++ {
		rules = append(rules, Rule{Host: fmt.Sprintf("site%03d.example.com", i), Rule: RuleRead})
	}
	_, err := ValidatePut(PutRequest{ExpectedRevision: &revision, Mode: ModeOpen, Rules: rules})
	assertCode(t, err, CodeTooManyRules)

	_, err = ValidatePut(PutRequest{ExpectedRevision: &revision, Mode: ModeOpen, Rules: []Rule{{Host: "example.com", Rule: RuleBlock, Note: repeat('n', MaxNoteLength+1)}}})
	assertCode(t, err, CodeNoteTooLong)
}

func TestValidatePrefBoundsItsLifetime(t *testing.T) {
	now := time.Date(2026, 10, 24, 12, 0, 0, 0, time.UTC)
	revision := int64(1)

	input, err := ValidatePref(PrefRequest{ExpectedRevision: &revision, Level: LevelAct}, now)
	if err != nil {
		t.Fatalf("a choice with no expiry must pass: %v", err)
	}
	if !input.ExpiresAt.Equal(now.Add(MaxPrefLifetime)) {
		t.Fatalf("an absent expiry must default to 90 days from now, got %s", input.ExpiresAt)
	}

	past := now.Add(-time.Minute)
	_, err = ValidatePref(PrefRequest{ExpectedRevision: &revision, Level: LevelRead, ExpiresAt: &past}, now)
	assertCode(t, err, CodeExpiryOutOfRange)

	beyond := now.Add(MaxPrefLifetime + time.Second)
	_, err = ValidatePref(PrefRequest{ExpectedRevision: &revision, Level: LevelRead, ExpiresAt: &beyond}, now)
	assertCode(t, err, CodeExpiryOutOfRange)

	_, err = ValidatePref(PrefRequest{ExpectedRevision: &revision, Level: "always"}, now)
	assertCode(t, err, CodeInvalidLevel)

	_, err = ValidatePref(PrefRequest{Level: LevelRead}, now)
	assertCode(t, err, CodeExpectedRevisionRequired)
}

func assertCode(t *testing.T, err error, code string) {
	t.Helper()
	var invalid *ValidationError
	if !errors.As(err, &invalid) {
		t.Fatalf("want a ValidationError with %s, got %v", code, err)
	}
	for _, field := range invalid.Errors {
		if field.Code == code {
			return
		}
	}
	t.Fatalf("want code %s, got %+v", code, invalid.Errors)
}

func repeat(c byte, n int) string {
	out := make([]byte, n)
	for i := range out {
		out[i] = c
	}
	return string(out)
}
