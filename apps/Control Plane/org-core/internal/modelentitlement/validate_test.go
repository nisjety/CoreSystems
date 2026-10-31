package modelentitlement

import (
	"errors"
	"fmt"
	"os"
	"reflect"
	"regexp"
	"sort"
	"strings"
	"testing"
)

func revision(value int64) *int64 { return &value }

func validPolicy() Policy {
	return Policy{AllowMode: AllowModeListed, Allow: []string{"gpt-5-mini"}, Deny: []string{}}
}

func validationCodes(t *testing.T, err error) []string {
	t.Helper()
	var invalid *ValidationError
	if !errors.As(err, &invalid) {
		t.Fatalf("want a ValidationError, got %v", err)
	}
	codes := make([]string, 0, len(invalid.Errors))
	for _, fieldErr := range invalid.Errors {
		codes = append(codes, fieldErr.Path+"="+fieldErr.Code)
	}
	sort.Strings(codes)
	return codes
}

func TestValidatePutRequiresAnExpectedRevision(t *testing.T) {
	if _, err := ValidatePut(PutRequest{}); !reflect.DeepEqual(validationCodes(t, err), []string{"expected_revision=expected_revision_required"}) {
		t.Fatalf("a body with no expected_revision must be refused, got %v", err)
	}
	if _, err := ValidatePut(PutRequest{ExpectedRevision: revision(-1)}); err == nil {
		t.Fatal("a negative expected_revision must be refused")
	}
	if _, err := ValidatePut(PutRequest{ExpectedRevision: revision(0)}); err != nil {
		t.Fatalf("an empty set at revision 0 is a valid write (it clears the policy): %v", err)
	}
}

func TestValidatePutNormalisesEntries(t *testing.T) {
	got, err := ValidatePut(PutRequest{
		ExpectedRevision: revision(2),
		OrgDefault: &Policy{
			AllowMode: " Listed ",
			Allow:     []string{" GPT-5-Mini ", "provider:Anthropic", "gpt-5-mini", "claude-sonnet-4-5"},
			Deny:      []string{"O4-Mini"},
			MaxEffort: " HIGH ",
		},
	})
	if err != nil {
		t.Fatalf("valid put: %v", err)
	}
	want := Policy{
		AllowMode: AllowModeListed,
		Allow:     []string{"claude-sonnet-4-5", "gpt-5-mini", "provider:anthropic"},
		Deny:      []string{"o4-mini"},
		MaxEffort: "high",
	}
	if !reflect.DeepEqual(*got.OrgDefault, want) {
		t.Fatalf("normalised policy = %+v; want %+v", *got.OrgDefault, want)
	}
	if got.Roles == nil {
		t.Fatal("roles must normalise to an empty map, never nil, so the store writes no null")
	}
}

func TestValidateEntries(t *testing.T) {
	cases := []struct {
		entry string
		want  string // "" is accepted
	}{
		{"gpt-5-mini", ""},
		{"claude-sonnet-4-5", ""},
		{"meta/llama-3.1:8b", ""},
		{"provider:azure-openai", ""},
		{"provider:", CodeInvalidEntry},
		{"provider:Bad Provider", CodeInvalidEntry},
		{"", CodeEmptyEntry},
		{"   ", CodeEmptyEntry},
		{"gpt-*", CodeGlobNotAllowed},
		{"*", CodeGlobNotAllowed},
		{"gpt-?", CodeGlobNotAllowed},
		{"gpt-[45]", CodeGlobNotAllowed},
		{"provider:*", CodeGlobNotAllowed},
		{"verevon-genius", CodeIntentIDNotAllowed},
		{"VEREVON-Budget ", CodeIntentIDNotAllowed},
		{"acme-verevon-genius", ""},
		{"provider:verevon-core", ""},
		{"gpt 5", CodeInvalidEntry},
		{"-gpt", CodeInvalidEntry},
		{"gpt\n5", CodeInvalidEntry},
		{strings.Repeat("a", MaxEntryLength), ""},
		{strings.Repeat("a", MaxEntryLength+1), CodeEntryTooLong},
	}
	for _, tc := range cases {
		_, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &Policy{AllowMode: AllowModeListed, Allow: []string{tc.entry}}})
		got := ""
		if err != nil {
			codes := validationCodes(t, err)
			if len(codes) != 1 {
				t.Fatalf("entry %q gave %v", tc.entry, codes)
			}
			got = strings.TrimPrefix(codes[0], "org_default.allow[0]=")
		}
		if got != tc.want {
			t.Errorf("entry %q: code %q; want %q", tc.entry, got, tc.want)
		}
	}
}

// An intent mode is judged on the model it resolves to, so a deny entry naming
// one is refused just as an allow entry is, and a role's list follows the same
// rule. A surface default may still name an intent, because no list can name one
// and the model it resolves to is judged at admission.
func TestValidateRefusesIntentIDsOnDenyAndRoleLists(t *testing.T) {
	cases := []struct {
		name string
		req  PutRequest
		want []string // nil is accepted
	}{
		{
			"intent in deny",
			PutRequest{ExpectedRevision: revision(0), OrgDefault: &Policy{AllowMode: AllowModeAll, Deny: []string{"verevon-genius"}}},
			[]string{"org_default.deny[0]=" + CodeIntentIDNotAllowed},
		},
		{
			"intent in a role's allow list",
			PutRequest{ExpectedRevision: revision(0), Roles: map[string]Policy{"X": {AllowMode: AllowModeListed, Allow: []string{"gpt-5-mini", "verevon-genius"}}}},
			[]string{"roles.x.allow[1]=" + CodeIntentIDNotAllowed},
		},
		{
			"a listed intent is refused, and its default is not refused on top",
			PutRequest{ExpectedRevision: revision(0), OrgDefault: &Policy{AllowMode: AllowModeListed, Allow: []string{"verevon-balance"}, Defaults: map[string]SurfaceDefault{"chat": {Model: "verevon-balance"}}}},
			[]string{"org_default.allow[0]=" + CodeIntentIDNotAllowed},
		},
		{
			"an intent default is accepted where the policy cannot list one",
			PutRequest{ExpectedRevision: revision(0), OrgDefault: &Policy{AllowMode: AllowModeListed, Allow: []string{"gpt-5-mini"}, Defaults: map[string]SurfaceDefault{"chat": {Model: "verevon-balance"}}}},
			nil,
		},
		{
			"an intent is a legitimate surface default in all mode",
			PutRequest{ExpectedRevision: revision(0), OrgDefault: &Policy{AllowMode: AllowModeAll, Defaults: map[string]SurfaceDefault{"chat": {Model: "verevon-balance"}}}},
			nil,
		},
	}
	for _, tc := range cases {
		_, err := ValidatePut(tc.req)
		if tc.want == nil {
			if err != nil {
				t.Errorf("%s: %v", tc.name, err)
			}
			continue
		}
		if !reflect.DeepEqual(validationCodes(t, err), tc.want) {
			t.Errorf("%s: got %v; want %v", tc.name, err, tc.want)
		}
	}

	_, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &Policy{AllowMode: AllowModeListed, Allow: []string{"verevon-genius"}}})
	var invalid *ValidationError
	if !errors.As(err, &invalid) || len(invalid.Errors) != 1 || !strings.Contains(invalid.Errors[0].Message, "not a model") {
		t.Fatalf("the refusal must say that an intent mode is not a model, got %v", err)
	}
}

func TestValidateLimits(t *testing.T) {
	many := make([]string, MaxEntries+1)
	for index := range many {
		many[index] = fmt.Sprintf("model-%d", index)
	}
	_, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &Policy{AllowMode: AllowModeListed, Allow: many}})
	if !reflect.DeepEqual(validationCodes(t, err), []string{"org_default.allow=too_many_entries"}) {
		t.Fatalf("501 entries must be refused, got %v", err)
	}
	if _, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &Policy{AllowMode: AllowModeListed, Allow: many[:MaxEntries]}}); err != nil {
		t.Fatalf("exactly %d entries must be accepted: %v", MaxEntries, err)
	}

	roles := map[string]Policy{}
	for index := 0; index <= MaxRoles; index++ {
		roles[fmt.Sprintf("role-%d", index)] = validPolicy()
	}
	_, err = ValidatePut(PutRequest{ExpectedRevision: revision(0), Roles: roles})
	if !reflect.DeepEqual(validationCodes(t, err), []string{"roles=too_many_roles"}) {
		t.Fatalf("21 roles must be refused, got %v", err)
	}
	delete(roles, "role-0")
	if _, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), Roles: roles}); err != nil {
		t.Fatalf("exactly %d roles must be accepted: %v", MaxRoles, err)
	}
}

func TestValidateAllowMode(t *testing.T) {
	cases := []struct {
		name   string
		policy Policy
		want   []string
	}{
		{"missing mode", Policy{}, []string{"org_default.allow_mode=invalid_allow_mode"}},
		{"unknown mode", Policy{AllowMode: "some"}, []string{"org_default.allow_mode=invalid_allow_mode"}},
		{"all with an allow list", Policy{AllowMode: AllowModeAll, Allow: []string{"gpt-5"}}, []string{"org_default.allow=allow_requires_listed_mode"}},
		{"all with a deny list", Policy{AllowMode: AllowModeAll, Deny: []string{"gpt-5"}}, nil},
		{"listed with nothing is a valid block-all", Policy{AllowMode: AllowModeListed}, nil},
	}
	for _, tc := range cases {
		_, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &tc.policy})
		if tc.want == nil {
			if err != nil {
				t.Errorf("%s: %v", tc.name, err)
			}
			continue
		}
		if !reflect.DeepEqual(validationCodes(t, err), tc.want) {
			t.Errorf("%s: got %v; want %v", tc.name, err, tc.want)
		}
	}
}

func TestValidateEffortAndTier(t *testing.T) {
	for _, effort := range []string{"minimal", "low", "medium", "high", "xhigh", "max", " High "} {
		policy := validPolicy()
		policy.MaxEffort = effort
		if _, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &policy}); err != nil {
			t.Errorf("max_effort %q must be accepted: %v", effort, err)
		}
	}
	for _, effort := range []string{"quick", "deep", "ultra", "0"} {
		policy := validPolicy()
		policy.MaxEffort = effort
		_, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &policy})
		if !reflect.DeepEqual(validationCodes(t, err), []string{"org_default.max_effort=unknown_effort"}) {
			t.Errorf("max_effort %q must be refused as not a rung, got %v", effort, err)
		}
	}
	for _, tier := range []string{"global", "eu_resident", "zdr_contractual", "sovereign"} {
		policy := validPolicy()
		policy.MinPrivacyTier = tier
		if _, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &policy}); err != nil {
			t.Errorf("min_privacy_tier %q must be accepted: %v", tier, err)
		}
	}
	for _, tier := range []string{"unspecified", "eu", "private"} {
		policy := validPolicy()
		policy.MinPrivacyTier = tier
		_, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &policy})
		if !reflect.DeepEqual(validationCodes(t, err), []string{"org_default.min_privacy_tier=unknown_privacy_tier"}) {
			t.Errorf("min_privacy_tier %q must be refused, got %v", tier, err)
		}
	}
}

func TestValidateRoleNames(t *testing.T) {
	good, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), Roles: map[string]Policy{" Admin ": validPolicy(), "ops.team-2": validPolicy()}})
	if err != nil {
		t.Fatalf("valid role names: %v", err)
	}
	if _, ok := good.Roles["admin"]; !ok || len(good.Roles) != 2 {
		t.Fatalf("role keys must be lower-cased and trimmed, got %v", good.Roles)
	}
	for _, name := range []string{"", "   ", "-lead", "has space", "slash/role", strings.Repeat("r", 65)} {
		_, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), Roles: map[string]Policy{name: validPolicy()}})
		if len(validationCodes(t, err)) != 1 || !strings.HasSuffix(validationCodes(t, err)[0], "="+CodeInvalidRole) {
			t.Errorf("role name %q must be refused as invalid_role, got %v", name, err)
		}
	}
	_, err = ValidatePut(PutRequest{ExpectedRevision: revision(0), Roles: map[string]Policy{"Admin": validPolicy(), "admin": validPolicy()}})
	if len(validationCodes(t, err)) != 1 || !strings.HasSuffix(validationCodes(t, err)[0], "="+CodeDuplicateRole) {
		t.Fatalf("two names that fold to the same role must be refused, got %v", err)
	}
}

func TestValidateSurfaceDefaults(t *testing.T) {
	base := func(defaults map[string]SurfaceDefault, mutate func(*Policy)) error {
		policy := Policy{AllowMode: AllowModeListed, Allow: []string{"gpt-5-mini", "provider:anthropic"}, Deny: []string{"gpt-5"}, MaxEffort: "medium", Defaults: defaults}
		if mutate != nil {
			mutate(&policy)
		}
		_, err := ValidatePut(PutRequest{ExpectedRevision: revision(0), OrgDefault: &policy})
		return err
	}

	if err := base(map[string]SurfaceDefault{"chat": {Model: "gpt-5-mini", Effort: "medium"}}, nil); err != nil {
		t.Fatalf("a default inside its own policy must be accepted: %v", err)
	}
	if err := base(map[string]SurfaceDefault{"chat": {Model: "some-claude-model"}}, nil); err != nil {
		t.Fatalf("a model a provider selector may admit cannot be refused here, got %v", err)
	}

	cases := []struct {
		name     string
		defaults map[string]SurfaceDefault
		mutate   func(*Policy)
		want     string
	}{
		{"default model on the deny list", map[string]SurfaceDefault{"chat": {Model: "gpt-5"}}, nil, "org_default.defaults.chat.model=" + CodeDefaultModelNotPermitted},
		{"default model not on the allow list", map[string]SurfaceDefault{"chat": {Model: "o4-mini"}}, func(p *Policy) { p.Allow = []string{"gpt-5-mini"} }, "org_default.defaults.chat.model=" + CodeDefaultModelNotPermitted},
		{"default model is a selector", map[string]SurfaceDefault{"chat": {Model: "provider:anthropic"}}, nil, "org_default.defaults.chat.model=" + CodeDefaultModelIsSelector},
		{"default effort above the ceiling", map[string]SurfaceDefault{"chat": {Effort: "high"}}, nil, "org_default.defaults.chat.effort=" + CodeDefaultEffortAboveMax},
		{"default effort is not a rung", map[string]SurfaceDefault{"chat": {Effort: "deep"}}, nil, "org_default.defaults.chat.effort=" + CodeUnknownEffort},
		{"empty surface default", map[string]SurfaceDefault{"chat": {}}, nil, "org_default.defaults.chat=" + CodeEmptySurfaceDefault},
		{"bad surface name", map[string]SurfaceDefault{"Chat Room": {Model: "gpt-5-mini"}}, nil, "org_default.defaults.Chat Room=" + CodeInvalidSurface},
	}
	for _, tc := range cases {
		if got := validationCodes(t, base(tc.defaults, tc.mutate)); !reflect.DeepEqual(got, []string{tc.want}) {
			t.Errorf("%s: got %v; want [%s]", tc.name, got, tc.want)
		}
	}

	tooMany := map[string]SurfaceDefault{}
	for index := 0; index <= MaxSurfaces; index++ {
		tooMany[fmt.Sprintf("surface-%d", index)] = SurfaceDefault{Model: "gpt-5-mini"}
	}
	if got := validationCodes(t, base(tooMany, nil)); !reflect.DeepEqual(got, []string{"org_default.defaults=" + CodeTooManySurfaces}) {
		t.Errorf("17 surfaces: got %v", got)
	}
}

func TestValidateReportsEveryProblemAtOnce(t *testing.T) {
	_, err := ValidatePut(PutRequest{
		OrgDefault: &Policy{AllowMode: "x", Allow: []string{"a*"}, MaxEffort: "ultra"},
		Roles:      map[string]Policy{"bad name": validPolicy()},
	})
	if got := validationCodes(t, err); len(got) != 5 {
		t.Fatalf("an admin must see every problem in one response, got %v", got)
	}
}

// The effort ladder lives in inference-core (WS11); org-core keeps a copy only to
// validate max_effort. This pins the copy to the Rust source when the repository
// is present, and skips inside the module-only Docker build context.
func TestEffortLadderMatchesInferenceCore(t *testing.T) {
	const path = "../../../../Model Plane/rust/services/inference-core/src/provider/effort.rs"
	source, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) && os.Getenv("REQUIRE_CONTRACT") == "" {
			t.Skipf("%s is absent (set REQUIRE_CONTRACT=1 to make that a failure)", path)
		}
		t.Fatalf("read %s: %v", path, err)
	}
	text := strings.ReplaceAll(string(source), "\r\n", "\n")
	start := strings.Index(text, "pub fn as_str(self)")
	end := strings.Index(text, "pub fn from_name")
	if start < 0 || end < start {
		t.Fatal("effort.rs no longer has as_str followed by from_name; update this test with the ladder module")
	}
	var rust []string
	for _, match := range regexp.MustCompile(`Level::\w+ => "(\w+)"`).FindAllStringSubmatch(text[start:end], -1) {
		rust = append(rust, match[1])
	}
	var ours []string
	for name := range effortRank {
		ours = append(ours, name)
	}
	sort.Strings(rust)
	sort.Strings(ours)
	if !reflect.DeepEqual(rust, ours) {
		t.Fatalf("org-core's effort rungs %v differ from inference-core's %v", ours, rust)
	}
}
