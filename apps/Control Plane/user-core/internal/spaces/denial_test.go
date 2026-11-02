package spaces

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

// WS03-S2: the typed denial vocabulary. Refusals carry a closed code next to
// the message they always had, so existing err.Error() assertions stay true.

func allGoodFlags() denialFlags {
	return denialFlags{
		OrgMember: true, SpaceFound: true, SpaceMember: true, SpaceKind: "room",
		RegistrationState: "active", HasPolicy: true, InAudience: true,
	}
}

func TestClassifyDenialTable(t *testing.T) {
	mutate := func(f func(*denialFlags)) denialFlags {
		flags := allGoodFlags()
		f(&flags)
		return flags
	}
	cases := []struct {
		name       string
		flags      denialFlags
		expect     ExpectKind
		wantCode   DenialCode
		wantDetail map[string]any
	}{
		{"non org member", mutate(func(f *denialFlags) { f.OrgMember = false }), ExpectAny, DenialMembershipRequired, map[string]any{}},
		{"unknown Space", mutate(func(f *denialFlags) { f.SpaceFound = false }), ExpectAny, DenialMembershipRequired, map[string]any{}},
		{"not a Space member", mutate(func(f *denialFlags) { f.SpaceMember = false }), ExpectAny, DenialMembershipRequired, map[string]any{}},
		{"not a member of an unregistered Space", denialFlags{}, ExpectShared, DenialMembershipRequired, map[string]any{}},
		{"non member of a draining Space still gets membership", mutate(func(f *denialFlags) { f.SpaceMember = false; f.RegistrationState = "deleting" }), ExpectAny, DenialMembershipRequired, map[string]any{}},
		{"not active carries state", mutate(func(f *denialFlags) { f.RegistrationState = "deleting" }), ExpectAny, DenialNotActive, map[string]any{"state": "deleting"}},
		{"personal decision on a room", allGoodFlags(), ExpectPersonal, DenialKindUnsupported, map[string]any{"kind": "room"}},
		{"shared decision on a personal Space", mutate(func(f *denialFlags) { f.SpaceKind = "personal" }), ExpectShared, DenialKindUnsupported, map[string]any{"kind": "personal"}},
		{"policy not provisioned", mutate(func(f *denialFlags) { f.HasPolicy = false }), ExpectAny, DenialPolicyNotProvisioned, map[string]any{}},
		{"shared and not in the audience", mutate(func(f *denialFlags) { f.InAudience = false }), ExpectShared, DenialAudienceNotCurrent, map[string]any{}},
		{"audience is only judged for shared", mutate(func(f *denialFlags) { f.SpaceKind = "personal"; f.InAudience = false }), ExpectPersonal, DenialAuthorityIncomplete, map[string]any{}},
		{"everything resolves, nothing else to say", allGoodFlags(), ExpectShared, DenialAuthorityIncomplete, map[string]any{}},
		{"any kind, shared, audience missing", mutate(func(f *denialFlags) { f.InAudience = false }), ExpectAny, DenialAudienceNotCurrent, map[string]any{}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			denial := classifyDenial(tc.flags, tc.expect)
			if denial == nil || denial.Code != tc.wantCode {
				t.Fatalf("classifyDenial = %+v, want code %s", denial, tc.wantCode)
			}
			if got := denial.Details(); !reflect.DeepEqual(got, tc.wantDetail) {
				t.Fatalf("details = %v, want %v", got, tc.wantDetail)
			}
		})
	}
}

func TestClassifyDenialIsAntiEnumeratingForNonMembers(t *testing.T) {
	// A non-member must get a byte-identical denial whatever the Space is like:
	// unknown, active, deleting, policyless, personal or room.
	base := classifyDenial(denialFlags{}, ExpectAny)
	for _, flags := range []denialFlags{
		{OrgMember: true, SpaceFound: true, SpaceMember: false, SpaceKind: "room", RegistrationState: "active", HasPolicy: true, InAudience: true},
		{OrgMember: true, SpaceFound: true, SpaceMember: false, SpaceKind: "personal", RegistrationState: "deleting"},
		{OrgMember: false, SpaceFound: true, SpaceMember: true, SpaceKind: "room", RegistrationState: "active"},
		{OrgMember: true, SpaceFound: false},
	} {
		for _, expect := range []ExpectKind{ExpectAny, ExpectPersonal, ExpectShared, ExpectMembershipOnly} {
			got := classifyDenial(flags, expect)
			if got.Code != base.Code || got.Error() != base.Error() || !reflect.DeepEqual(got.Details(), base.Details()) ||
				got.Status() != base.Status() || got.ResolvableBy() != base.ResolvableBy() {
				t.Fatalf("non-member denial leaks Space facts (%+v / %s): %+v vs %+v", flags, expect, got, base)
			}
		}
	}
}

func TestClassifyDenialMembershipOnlyNeverReachesPolicyCodes(t *testing.T) {
	for _, flags := range []denialFlags{
		allGoodFlags(),
		func() denialFlags { f := allGoodFlags(); f.HasPolicy = false; return f }(),
		func() denialFlags { f := allGoodFlags(); f.InAudience = false; return f }(),
		func() denialFlags { f := allGoodFlags(); f.SpaceKind = "personal"; return f }(),
	} {
		denial := classifyDenial(flags, ExpectMembershipOnly)
		switch denial.Code {
		case DenialMembershipRequired, DenialNotActive:
		default:
			t.Fatalf("membership-only classification reached %s", denial.Code)
		}
	}
}

func TestDenialCatalogueIsClosedAndWellFormed(t *testing.T) {
	seen := map[DenialCode]bool{}
	known := map[string]bool{"entitlement": true, "required_roles": true, "state": true, "kind": true, "effect": true}
	for _, spec := range DenialCatalogue() {
		if seen[spec.Code] {
			t.Errorf("duplicate code %s", spec.Code)
		}
		seen[spec.Code] = true
		if !strings.HasPrefix(string(spec.Code), "space_") {
			t.Errorf("code %s lacks the space_ prefix", spec.Code)
		}
		if spec.Status != http.StatusForbidden && spec.Status != http.StatusServiceUnavailable {
			t.Errorf("code %s has status %d", spec.Code, spec.Status)
		}
		if (spec.Status == http.StatusServiceUnavailable) != spec.Retryable && spec.Code == DenialAuthorityUnavailable {
			t.Errorf("space_authority_unavailable must be a retryable 503")
		}
		for _, key := range spec.DetailKeys {
			if !known[key] {
				t.Errorf("code %s allowlists unknown detail key %q", spec.Code, key)
			}
		}
	}
	if len(seen) != 10 {
		t.Errorf("vocabulary has %d codes, the plan fixes 10", len(seen))
	}
	// The copy is not the live catalogue.
	copyOf := DenialCatalogue()
	copyOf[0].Code = "mutated"
	if DenialCatalogue()[0].Code == "mutated" {
		t.Fatal("DenialCatalogue exposed the live catalogue")
	}
}

func TestDenialDetailsEmitsOnlyAllowlistedKeys(t *testing.T) {
	// Every field populated on every code: only that code's allowlisted keys may surface.
	for _, spec := range DenialCatalogue() {
		denial := &Denial{
			Code: spec.Code, Entitlement: "thread_read", RequiredRoles: []string{"owner"},
			State: "deleting", Kind: "room", Effect: "import_write", message: "m",
		}
		details := denial.Details()
		allowed := map[string]bool{}
		for _, key := range spec.DetailKeys {
			allowed[key] = true
		}
		for key := range details {
			if !allowed[key] {
				t.Errorf("code %s leaked detail key %q", spec.Code, key)
			}
		}
		if len(details) != len(spec.DetailKeys) {
			t.Errorf("code %s emitted %v, allowlist is %v", spec.Code, details, spec.DetailKeys)
		}
	}
	// Empty values are omitted, and required_roles is a copy.
	empty := (&Denial{Code: DenialRoleBelowFloor}).Details()
	if len(empty) != 0 {
		t.Fatalf("empty roles surfaced: %v", empty)
	}
	d := roleBelowFloorDenial("m", "editor", "owner")
	roles := d.Details()["required_roles"].([]string)
	roles[0] = "mutated"
	if d.RequiredRoles[0] != "editor" {
		t.Fatal("Details exposed the denial's own slice")
	}
	if got := (&Denial{Code: "space_made_up", Entitlement: "x"}).Details(); len(got) != 0 {
		t.Fatalf("unknown code produced details %v", got)
	}
}

func TestDenialStatusAndResolvableByFollowTheCatalogue(t *testing.T) {
	for _, spec := range DenialCatalogue() {
		d := &Denial{Code: spec.Code}
		if d.Status() != spec.Status || d.ResolvableBy() != spec.ResolvableBy {
			t.Errorf("%s: status/resolvable_by %d/%s, want %d/%s", spec.Code, d.Status(), d.ResolvableBy(), spec.Status, spec.ResolvableBy)
		}
	}
	unknown := &Denial{Code: "space_made_up"}
	if unknown.Status() != http.StatusForbidden || unknown.ResolvableBy() != ResolvableByOperator {
		t.Fatalf("unknown code did not fail to forbidden/operator: %d %s", unknown.Status(), unknown.ResolvableBy())
	}
	var nilDenial *Denial
	if nilDenial.Error() != "" {
		t.Fatal("nil Denial must render an empty message")
	}
	if AuthorityUnavailableDenial("x").Status() != http.StatusServiceUnavailable {
		t.Fatal("authority unavailable must be a 503")
	}
	if _, ok := AsDenial(nil); ok {
		t.Fatal("AsDenial(nil) reported a denial")
	}
	if _, ok := AsDenial(fmt.Errorf("plain")); ok {
		t.Fatal("AsDenial on a plain error reported a denial")
	}
	wrapped := fmt.Errorf("outer: %w", entitlementOffDenial("import_write", "inner"))
	if d, ok := AsDenial(wrapped); !ok || d.Code != DenialEntitlementOff || d.Entitlement != "import_write" {
		t.Fatalf("AsDenial through %%w = %+v, %v", d, ok)
	}
}

// expectDenial asserts a validator/issuer returned a typed denial with the
// code and the exact historical message.
func expectDenial(t *testing.T, err error, code DenialCode, message string) *Denial {
	t.Helper()
	if err == nil {
		t.Fatalf("no refusal, want %s %q", code, message)
	}
	denial, ok := AsDenial(err)
	if !ok {
		t.Fatalf("refusal %q is not a typed denial", err)
	}
	if denial.Code != code {
		t.Fatalf("refusal %q: code %s, want %s", err, denial.Code, code)
	}
	if err.Error() != message {
		t.Fatalf("refusal text changed:\n got %q\nwant %q", err.Error(), message)
	}
	return denial
}

func TestValidatorsReturnTypedDenialsAndKeepTheirExactText(t *testing.T) {
	now := time.Date(2026, 10, 27, 12, 0, 0, 0, time.UTC)

	// Personal create: role floor and entitlement.
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Role = "viewer"
	d := expectDenial(t, evidence.Validate(), DenialRoleBelowFloor, `space role "viewer" cannot create a thread`)
	if !reflect.DeepEqual(d.RequiredRoles, []string{"editor", "manager", "owner"}) {
		t.Fatalf("required roles = %v", d.RequiredRoles)
	}
	evidence = validPersonalThreadEvidence()
	evidence.ThreadCreateEntitled = false
	d = expectDenial(t, evidence.Validate(), DenialEntitlementOff, "thread creation entitlement is not active")
	if d.Entitlement != "thread_create" {
		t.Fatalf("entitlement = %q", d.Entitlement)
	}

	// Shared decisions refuse a personal Space with the kind named.
	personal := validPersonalThreadEvidence()
	d = expectDenial(t, personal.ValidateForSharedThread(), DenialKindUnsupported, "shared thread decision requires a non-personal Space")
	if d.Kind != "personal" {
		t.Fatalf("kind = %q", d.Kind)
	}
	// ...and the personal-only decisions refuse a room.
	room := validSharedThreadReadEvidence()
	expectDenial(t, room.ValidateForOwnScheduledThreadRead(), DenialKindUnsupported, "this decision is for a personal Space only")

	// Shared, missing audience: authority incomplete.
	shared := validSharedThreadReadEvidence()
	shared.RecipientAudienceRef = ""
	expectDenial(t, shared.ValidateForSharedThreadRead(), DenialAuthorityIncomplete, "shared Space recipient audience is required")

	// Shared read entitlement.
	shared = validSharedThreadReadEvidence()
	shared.ThreadReadEntitled = false
	d = expectDenial(t, shared.ValidateForSharedThreadRead(), DenialEntitlementOff, "shared Space thread read authority is incomplete")
	if d.Entitlement != "thread_read" {
		t.Fatalf("entitlement = %q", d.Entitlement)
	}

	// Own scheduled read: owner only.
	own := validOwnScheduledThreadReadEvidence()
	own.Membership.Role = "editor"
	expectDenial(t, own.ValidateForOwnScheduledThreadRead(), DenialRoleBelowFloor, "only the Space's own owner may read it")

	// ZDR refusals name the effect.
	zdr := validPersonalImportEvidence()
	zdr.Privacy.ZeroDataRetention = true
	_, err := IssuePersonalImportDecision(zdr, PersonalImportDecisionRequest{DecisionRef: "d", IdempotencyKey: "i", SourceType: "notion", Nonce: "n"}, now)
	d = expectDenial(t, err, DenialZDRForbidsEffect, "zero data retention forbids durable import")
	if d.Effect != "import_write" {
		t.Fatalf("effect = %q", d.Effect)
	}
	sched := validPersonalThreadEvidence()
	sched.ScheduleFireEntitled = true
	sched.Privacy.ZeroDataRetention = true
	_, err = IssueScheduleCreateDecision(sched, validScheduleCreateRequest(), now)
	d = expectDenial(t, err, DenialZDRForbidsEffect, "zero data retention forbids durable schedule creation")
	if d.Effect != "schedule_create" {
		t.Fatalf("effect = %q", d.Effect)
	}
	sched = validPersonalThreadEvidence()
	_, err = IssueScheduleCreateDecision(sched, validScheduleCreateRequest(), now)
	expectDenial(t, err, DenialEntitlementOff, "schedule creation entitlement is not active")

	// Owner grant: a role floor, the entitlement, then ZDR, each with the one historical message.
	const ownerGrantRefusal = "current personal Space authority cannot manage agent grants"
	grant := validPersonalThreadEvidence()
	grant.AgentActionEntitled = true
	grant.Membership.Role = "editor"
	_, err = IssueOwnerGrantDecision(grant, validOwnerGrantRequest(), now)
	expectDenial(t, err, DenialRoleBelowFloor, ownerGrantRefusal)
	grant.Membership.Role = "owner"
	grant.AgentActionEntitled = false
	_, err = IssueOwnerGrantDecision(grant, validOwnerGrantRequest(), now)
	expectDenial(t, err, DenialEntitlementOff, ownerGrantRefusal)
	grant.AgentActionEntitled = true
	grant.Privacy.ZeroDataRetention = true
	_, err = IssueOwnerGrantDecision(grant, validOwnerGrantRequest(), now)
	d = expectDenial(t, err, DenialZDRForbidsEffect, ownerGrantRefusal)
	if d.Effect != "owner_grant" {
		t.Fatalf("effect = %q", d.Effect)
	}
}

func TestSpaceDenialCodesAreNotAMessageContract(t *testing.T) {
	// Messages are unchanged text; the code is the contract. A refusal that is
	// not a vocabulary member (a plain validation error) must stay untyped, so
	// callers fall back to the unchanged 403.
	_, err := IssueSharedThreadReadDecision(validSharedThreadReadEvidence(), SharedThreadReadDecisionRequest{
		DecisionRef: "", IdempotencyKey: "i", Nonce: "n",
	}, time.Now())
	if err == nil {
		t.Fatal("blank decision_ref was issued")
	}
	if _, ok := AsDenial(err); ok {
		t.Fatalf("a request-shape error was typed as a Space denial: %v", err)
	}
}

func TestDenialCodesGolden(t *testing.T) {
	golden, shaHex, err := DenialCodesGolden()
	if err != nil {
		t.Fatalf("DenialCodesGolden: %v", err)
	}
	goldenPath := filepath.Join("testdata", "space-denial-codes.golden.json")
	pinPath := goldenPath + ".sha256"
	pin := shaHex + "  space-denial-codes.golden.json\n"

	if os.Getenv("UPDATE_CONTRACT_GOLDEN") == "1" {
		if err := os.WriteFile(goldenPath, golden, 0o644); err != nil {
			t.Fatalf("write golden: %v", err)
		}
		if err := os.WriteFile(pinPath, []byte(pin), 0o644); err != nil {
			t.Fatalf("write pin: %v", err)
		}
		t.Log("rewrote the denial codes golden and its sha256 pin (UPDATE_CONTRACT_GOLDEN=1)")
		return
	}

	// autocrlf rewrites the checkout on Windows; the contract is the LF bytes.
	onDisk, err := os.ReadFile(goldenPath)
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	onDisk = bytes.ReplaceAll(onDisk, []byte("\r\n"), []byte("\n"))
	if !bytes.Equal(onDisk, golden) {
		t.Fatalf("testdata/space-denial-codes.golden.json is stale against the denial catalogue;\n"+
			"if the vocabulary change is intended, regenerate with UPDATE_CONTRACT_GOLDEN=1\n got:\n%s\nwant:\n%s", onDisk, golden)
	}
	sum := sha256.Sum256(onDisk)
	if hex.EncodeToString(sum[:]) != shaHex {
		t.Fatalf("golden bytes do not hash to the catalogue's sha256")
	}
	rawPin, err := os.ReadFile(pinPath)
	if err != nil {
		t.Fatalf("read pin: %v", err)
	}
	if got := strings.ReplaceAll(string(rawPin), "\r\n", "\n"); got != pin {
		t.Fatalf("space-denial-codes.golden.json.sha256 is stale:\n got %q\nwant %q", got, pin)
	}
}
