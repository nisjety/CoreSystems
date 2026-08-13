package spaces

import (
	"encoding/json"
	"os"
	"strings"
	"testing"
)

func TestValidateRegistrationRejectsUnscopedOrUnknownSpaces(t *testing.T) {
	for _, registration := range []Registration{
		{SpaceRef: "", OrgID: "org-1", Kind: KindPersonal, LifecycleRevision: 1},
		{SpaceRef: "space-1", OrgID: "", Kind: KindPersonal, LifecycleRevision: 1},
		{SpaceRef: "space-1", OrgID: "org-1", Kind: "unknown", LifecycleRevision: 1},
		{SpaceRef: "space-1", OrgID: "org-1", Kind: KindPersonal, LifecycleRevision: 0},
	} {
		if err := registration.Validate(); err == nil {
			t.Fatalf("Validate(%+v) unexpectedly succeeded", registration)
		}
	}
}

func TestValidateRegistrationRequiresAStableOwnerPrincipal(t *testing.T) {
	if err := (Registration{SpaceRef: "space-1", OrgID: "org-1", Kind: KindPersonal, LifecycleRevision: 1}).Validate(); err == nil {
		t.Fatal("registration without owner principal unexpectedly succeeded")
	}
	if err := (Registration{SpaceRef: "space-1", OrgID: "org-1", OwnerPrincipalID: "user-1", Kind: KindPersonal, Lifecycle: LifecyclePendingRegistration, LifecycleRevision: 1}).Validate(); err != nil {
		t.Fatalf("complete registration rejected: %v", err)
	}
}

func TestRegistrationLifecycleIsExplicitAndCannotInventAnUnknownState(t *testing.T) {
	valid := Registration{
		SpaceRef: "space-1", OrgID: "org-1", OwnerPrincipalID: "user-1",
		Kind: KindPersonal, Lifecycle: LifecycleDeleting, LifecycleRevision: 2,
	}
	if err := valid.Validate(); err != nil {
		t.Fatalf("deleting lifecycle registration rejected: %v", err)
	}
	valid.Lifecycle = SpaceLifecycle("forged")
	if err := valid.Validate(); err == nil {
		t.Fatal("unknown Application lifecycle was accepted")
	}
}

func TestRegistrationLifecycleMapsToFailClosedControlStates(t *testing.T) {
	for lifecycle, want := range map[SpaceLifecycle]string{
		LifecyclePendingRegistration: "active",
		LifecycleActive:              "active",
		LifecycleSuspended:           "suspended",
		LifecycleDeleting:            "deleting",
		LifecycleDeleted:             "deleted",
		LifecycleFailedRegistration:  "active",
	} {
		got, err := (Registration{Lifecycle: lifecycle}).RegistrationState()
		if err != nil || got != want {
			t.Fatalf("lifecycle %q mapped to %q, %v; want %q", lifecycle, got, err, want)
		}
	}
}

func TestDeletionAuthorizationRequestAndReceiptAreBoundToOneIntent(t *testing.T) {
	request := DeletionAuthorizationRequest{
		SpaceRef: "space-1", OrgID: "org-1", OwnerPrincipalID: "user-1",
		RequestID: "delete-1", IdempotencyKey: "idem-1",
	}
	if err := request.Validate(); err != nil {
		t.Fatalf("valid deletion request rejected: %v", err)
	}
	if err := (DeletionAuthorizationReceipt{RequestID: "delete-1", Status: DeletionBlockedLegalHold}).ValidateFor(request.RequestID); err != nil {
		t.Fatalf("legal hold receipt rejected: %v", err)
	}
	if err := (DeletionAuthorizationReceipt{RequestID: "other", Status: DeletionAuthorized}).ValidateFor(request.RequestID); err == nil {
		t.Fatal("receipt for another request accepted")
	}
}

func TestDeletionPolicyRequiresAnOrganizationAndKeepsRolloutExplicit(t *testing.T) {
	if err := (DeletionPolicy{}).Validate(); err == nil {
		t.Fatal("unscoped deletion policy accepted")
	}
	if err := (DeletionPolicy{OrgID: "org-1", DeletionEntitled: true, PersonalRolloutEnabled: true}).Validate(); err != nil {
		t.Fatalf("explicit personal rollout policy rejected: %v", err)
	}
}

func TestLegalHoldRequiresCanonicalSpaceAndExternalReference(t *testing.T) {
	for _, hold := range []LegalHold{
		{},
		{SpaceRef: "space-1"},
		{HoldRef: "case-123"},
	} {
		if err := hold.Validate(); err == nil {
			t.Fatalf("incomplete legal hold accepted: %+v", hold)
		}
	}
	if err := (LegalHold{SpaceRef: "space-1", HoldRef: "case-123"}).Validate(); err != nil {
		t.Fatalf("complete legal hold rejected: %v", err)
	}
}

func TestRecipientAudienceRegistrationRequiresAnExactCanonicalCommitment(t *testing.T) {
	hash, err := RecipientAudienceHash("user-2", "user-1")
	if err != nil {
		t.Fatalf("RecipientAudienceHash: %v", err)
	}
	valid := RecipientAudienceRegistration{
		SpaceRef: "space-room", OrgID: "org-1", AudienceRef: "space:space-room:recipient-audience:1",
		AudienceHash: hash, Revision: 1, Recipients: []string{"user-2", "user-1"},
	}
	if err := valid.Validate(); err != nil {
		t.Fatalf("valid recipient registration rejected: %v", err)
	}
	for _, mutate := range []func(*RecipientAudienceRegistration){
		func(value *RecipientAudienceRegistration) { value.AudienceHash = "sha256:forged" },
		func(value *RecipientAudienceRegistration) { value.Recipients = []string{"user-1", "user-1"} },
		func(value *RecipientAudienceRegistration) { value.Revision = 0 },
	} {
		candidate := valid
		mutate(&candidate)
		if err := candidate.Validate(); err == nil {
			t.Fatal("invalid recipient audience registration unexpectedly accepted")
		}
	}
}

func TestAdvanceAuthorityRevisionInvalidatesEveryEffectClass(t *testing.T) {
	current := AuthorityRevision{Authority: 7, Membership: 3, Privacy: 5, RecipientAudience: 2, Entitlement: 1}
	next, err := current.Advance(ChangePrivacy)
	if err != nil {
		t.Fatalf("Advance: %v", err)
	}
	if next.Authority != 8 || next.Privacy != 6 || next.Membership != 3 || next.RecipientAudience != 2 || next.Entitlement != 1 {
		t.Fatalf("privacy advance = %+v, want only authority/privacy incremented", next)
	}

	next, err = next.Advance(ChangeRecipientAudience)
	if err != nil {
		t.Fatalf("Advance recipient audience: %v", err)
	}
	if next.Authority != 9 || next.RecipientAudience != 3 {
		t.Fatalf("recipient advance = %+v", next)
	}
}

func TestAdvanceAuthorityRevisionRejectsUnknownChangeAndInvalidState(t *testing.T) {
	if _, err := (AuthorityRevision{}).Advance(ChangeMembership); err == nil {
		t.Fatal("zero revision unexpectedly advanced")
	}
	if _, err := (AuthorityRevision{Authority: 1, Membership: 1, Privacy: 1, RecipientAudience: 1, Entitlement: 1}).Advance(AuthorityChange("unknown")); err == nil {
		t.Fatal("unknown authority change unexpectedly advanced")
	}
}

func TestCurrentMembershipRejectsInvalidRoleOrRevision(t *testing.T) {
	valid := CurrentMembership{
		SpaceRef: "space-1", OrgID: "org-1", SubjectID: "user-1", Kind: KindPersonal, Role: "owner",
		Revisions: AuthorityRevision{Authority: 1, Membership: 1, Privacy: 1, RecipientAudience: 1, Entitlement: 1},
	}
	if err := valid.Validate(); err != nil {
		t.Fatalf("valid current membership: %v", err)
	}
	invalidRole := valid
	invalidRole.Role = "admin"
	if err := invalidRole.Validate(); err == nil {
		t.Fatal("unknown role was accepted")
	}
	invalidRevision := valid
	invalidRevision.Revisions.Privacy = 0
	if err := invalidRevision.Validate(); err == nil {
		t.Fatal("incomplete revision was accepted")
	}
}

func TestCurrentMembershipUsesStableSnakeCaseWireFields(t *testing.T) {
	membership := CurrentMembership{
		SpaceRef: "space-1", OrgID: "org-1", SubjectID: "user-1", Kind: KindPersonal, Role: "owner",
		Revisions: AuthorityRevision{Authority: 1, Membership: 2, Privacy: 3, RecipientAudience: 4, Entitlement: 5},
	}
	body, err := json.Marshal(membership)
	if err != nil {
		t.Fatalf("marshal current membership: %v", err)
	}
	for _, field := range []string{
		`"space_ref":"space-1"`, `"org_id":"org-1"`, `"subject_id":"user-1"`,
		`"authority_revision":1`, `"recipient_audience_revision":4`,
	} {
		if !strings.Contains(string(body), field) {
			t.Fatalf("membership wire payload missing %s: %s", field, body)
		}
	}
}

func TestSpaceAuthorityMigrationCarriesRegisteredSpaceAndRevisionTables(t *testing.T) {
	contents, err := os.ReadFile("../../migrations/017_space_authority.up.sql")
	if err != nil {
		t.Fatalf("read Space authority migration: %v", err)
	}
	for _, required := range []string{
		"CREATE TABLE IF NOT EXISTS registered_spaces",
		"CREATE TABLE IF NOT EXISTS space_authority_revisions",
		"CREATE TABLE IF NOT EXISTS space_memberships",
		"owner_principal_id",
		"recipient_audience_revision",
		"entitlement_revision",
	} {
		if !strings.Contains(string(contents), required) {
			t.Fatalf("migration missing %q", required)
		}
	}
}

func TestSpaceEffectPolicyMigrationHasNoPermissiveDefault(t *testing.T) {
	contents, err := os.ReadFile("../../migrations/018_space_effect_policies.up.sql")
	if err != nil {
		t.Fatalf("read Space effect policy migration: %v", err)
	}
	for _, required := range []string{
		"CREATE TABLE IF NOT EXISTS space_effect_policies",
		"thread_create_entitled           BOOLEAN NOT NULL DEFAULT FALSE",
		"No default row is inserted",
	} {
		if !strings.Contains(string(contents), required) {
			t.Fatalf("effect-policy migration missing %q", required)
		}
	}
}

func TestImportEffectPolicyMigrationStaysDenyByDefault(t *testing.T) {
	contents, err := os.ReadFile("../../migrations/020_space_import_effect_policy.up.sql")
	if err != nil {
		t.Fatalf("read import-effect policy migration: %v", err)
	}
	for _, required := range []string{
		"ADD COLUMN IF NOT EXISTS import_write_entitled",
		"BOOLEAN NOT NULL DEFAULT FALSE",
		"Existing organizations stay denied",
	} {
		if !strings.Contains(string(contents), required) {
			t.Fatalf("import effect-policy migration missing %q", required)
		}
	}
}

func TestScheduleFireEffectPolicyMigrationStaysDenyByDefault(t *testing.T) {
	contents, err := os.ReadFile("../../migrations/021_space_schedule_fire_effect_policy.up.sql")
	if err != nil {
		t.Fatalf("read schedule-fire effect-policy migration: %v", err)
	}
	for _, required := range []string{
		"ADD COLUMN IF NOT EXISTS schedule_fire_entitled",
		"BOOLEAN NOT NULL DEFAULT FALSE",
		"fresh Control decision at each fire",
	} {
		if !strings.Contains(string(contents), required) {
			t.Fatalf("schedule-fire effect-policy migration missing %q", required)
		}
	}
}

func TestDeletionAuthorityMigrationIsDenyByDefaultAndKeepsLegalHoldsDurable(t *testing.T) {
	contents, err := os.ReadFile("../../migrations/022_space_deletion_authority.up.sql")
	if err != nil {
		t.Fatalf("read Space deletion authority migration: %v", err)
	}
	for _, required := range []string{
		"CREATE TABLE IF NOT EXISTS space_deletion_policies",
		"deletion_entitled",
		"BOOLEAN NOT NULL DEFAULT FALSE",
		"personal_rollout_enabled",
		"CREATE TABLE IF NOT EXISTS space_legal_holds",
		"CREATE TABLE IF NOT EXISTS space_deletion_requests",
		"blocked_legal_hold",
	} {
		if !strings.Contains(string(contents), required) {
			t.Fatalf("deletion authority migration missing %q", required)
		}
	}
}

func TestDeletionOperatorAuditMigrationIsAppendOnlyAndNarrowlyTyped(t *testing.T) {
	contents, err := os.ReadFile("../../migrations/023_space_deletion_operator_audit.up.sql")
	if err != nil {
		t.Fatalf("read Space deletion operator audit migration: %v", err)
	}
	for _, required := range []string{
		"CREATE TABLE IF NOT EXISTS space_deletion_operator_events",
		"policy_updated",
		"legal_hold_applied",
		"legal_hold_released",
		"actor_principal_id",
	} {
		if !strings.Contains(string(contents), required) {
			t.Fatalf("operator audit migration missing %q", required)
		}
	}
}

func TestControlDeletionPurgeKeepsOnlyReconciliationEvidence(t *testing.T) {
	contents, err := os.ReadFile("repository.go")
	if err != nil {
		t.Fatalf("read Space repository: %v", err)
	}
	for _, required := range []string{
		"func purgeControlSpaceProjections",
		"DELETE FROM space_memberships",
		"DELETE FROM space_recipient_audience_members",
		"DELETE FROM space_recipient_audiences",
		"registered reference, legal-hold row, deletion request",
	} {
		if !strings.Contains(string(contents), required) {
			t.Fatalf("Control deletion purge contract missing %q", required)
		}
	}
}

func TestEffectPolicyRejectsIncompleteProcessingFloor(t *testing.T) {
	policy := EffectPolicy{
		OrgID: "org-1", PrivacyPolicyRef: "privacy-1", Purpose: "assistant_collaboration",
		LawfulBasis: "contract", PrivacyClass: "internal", RetentionClass: "standard",
		Residency: "swedencentral", DeletionScope: "space",
	}
	if err := policy.Validate(); err != nil {
		t.Fatalf("complete policy rejected: %v", err)
	}
	policy.Residency = ""
	if err := policy.Validate(); err == nil {
		t.Fatal("incomplete policy was accepted")
	}
}
