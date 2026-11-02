package spaces

import (
	"errors"
	"reflect"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// WS03-S3: the org-admin entitlement writer and view. Pure logic first; the
// transactional behaviour is in org_policy_live_test.go.

func allEntitlements(value bool) map[string]bool {
	out := map[string]bool{}
	for _, spec := range EntitlementCatalog() {
		out[spec.Key] = value
	}
	return out
}

func withKeys(keys ...string) map[string]bool {
	out := allEntitlements(false)
	for _, key := range keys {
		out[key] = true
	}
	return out
}

func asOrgPolicyError(t *testing.T, err error) *OrgPolicyError {
	t.Helper()
	var policyErr *OrgPolicyError
	if !errors.As(err, &policyErr) {
		t.Fatalf("error %v is not an OrgPolicyError", err)
	}
	return policyErr
}

func TestValidateEntitlementChangeRequiresEveryKnownKeyAndNoOthers(t *testing.T) {
	var current EffectPolicy

	if err := ValidateEntitlementChange(current, allEntitlements(true)); err != nil {
		t.Fatalf("a full valid target was refused: %v", err)
	}

	// Unknown key: invalid_request naming the field.
	target := allEntitlements(false)
	target["teleport"] = true
	policyErr := asOrgPolicyError(t, ValidateEntitlementChange(current, target))
	if policyErr.Code != OrgPolicyCodeInvalidRequest || policyErr.Details["field"] != "entitlements.teleport" {
		t.Fatalf("unknown key refusal = %+v", policyErr)
	}

	// Missing key: invalid_request naming the first missing field in catalogue order.
	partial := allEntitlements(false)
	delete(partial, "retrieval_read")
	delete(partial, "process_registry")
	policyErr = asOrgPolicyError(t, ValidateEntitlementChange(current, partial))
	if policyErr.Code != OrgPolicyCodeInvalidRequest || policyErr.Details["field"] != "entitlements.retrieval_read" {
		t.Fatalf("missing key refusal = %+v", policyErr)
	}
	if err := ValidateEntitlementChange(current, nil); err == nil {
		t.Fatal("a nil target was accepted")
	}
}

func TestValidateEntitlementChangeUnderZDR(t *testing.T) {
	zdr := EffectPolicy{ZeroDataRetention: true}

	// Compatible bits can be enabled under ZDR.
	if err := ValidateEntitlementChange(zdr, withKeys("thread_create", "thread_read", "retrieval_read")); err != nil {
		t.Fatalf("zdr-compatible bits refused: %v", err)
	}

	// An inert bit cannot be newly enabled: conflict naming keys + reason.
	policyErr := asOrgPolicyError(t, ValidateEntitlementChange(zdr, withKeys("thread_create", "import_write")))
	if policyErr.Code != OrgPolicyCodeZDRConflict || policyErr.Details["reason"] != OrgPolicyReasonZDRInert ||
		!reflect.DeepEqual(policyErr.Details["keys"], []string{"import_write"}) {
		t.Fatalf("inert enable refusal = %+v", policyErr)
	}

	// An unproven bit: same conflict with the unproven reason.
	policyErr = asOrgPolicyError(t, ValidateEntitlementChange(zdr, withKeys("sandbox_capability")))
	if policyErr.Code != OrgPolicyCodeZDRConflict || policyErr.Details["reason"] != OrgPolicyReasonZDRUnproven ||
		!reflect.DeepEqual(policyErr.Details["keys"], []string{"sandbox_capability"}) {
		t.Fatalf("unproven enable refusal = %+v", policyErr)
	}

	// A mix reports every blocked key in catalogue order, and inert wins the reason.
	policyErr = asOrgPolicyError(t, ValidateEntitlementChange(zdr, withKeys("process_registry", "agent_action", "schedule_fire")))
	if !reflect.DeepEqual(policyErr.Details["keys"], []string{"agent_action", "schedule_fire", "process_registry"}) || policyErr.Details["reason"] != OrgPolicyReasonZDRInert {
		t.Fatalf("mixed refusal = %+v", policyErr)
	}

	// An already-enabled blocked bit left alone is not "newly enabled"; switching off is always fine.
	already := EffectPolicy{ZeroDataRetention: true, ImportWriteEntitled: true, SandboxCapabilityEntitled: true}
	if err := ValidateEntitlementChange(already, withKeys("import_write", "sandbox_capability")); err != nil {
		t.Fatalf("leaving an already enabled bit alone was refused: %v", err)
	}
	if err := ValidateEntitlementChange(already, allEntitlements(false)); err != nil {
		t.Fatalf("disabling was refused under ZDR: %v", err)
	}

	// Without ZDR nothing is blocked.
	if err := ValidateEntitlementChange(EffectPolicy{}, allEntitlements(true)); err != nil {
		t.Fatalf("non-ZDR org refused: %v", err)
	}
}

func TestPresetsAreCumulativeSubsetsOfTheCatalogue(t *testing.T) {
	wantOrder := []string{PresetLocked, PresetConversation, PresetConversationKnowledge, PresetAgentWork, PresetAgentComputer}
	presets := OrgPolicyPresets()
	var gotOrder []string
	for _, preset := range presets {
		gotOrder = append(gotOrder, preset.ID)
	}
	if !reflect.DeepEqual(gotOrder, wantOrder) {
		t.Fatalf("preset order = %v, want %v", gotOrder, wantOrder)
	}

	var previous map[string]bool
	for _, preset := range presets {
		for _, key := range preset.Keys {
			if _, ok := EntitlementSpecByKey(key); !ok {
				t.Errorf("preset %s names %q, which is not in the catalogue", preset.ID, key)
			}
		}
		expanded, ok := PresetEntitlements(preset.ID)
		if !ok {
			t.Fatalf("PresetEntitlements(%s) not found", preset.ID)
		}
		if len(expanded) != len(EntitlementCatalog()) {
			t.Errorf("preset %s expands to %d keys, want every catalogue key", preset.ID, len(expanded))
		}
		on := 0
		for _, value := range expanded {
			if value {
				on++
			}
		}
		if on != len(preset.Keys) {
			t.Errorf("preset %s turns %d keys on, lists %d", preset.ID, on, len(preset.Keys))
		}
		// Cumulative: everything the predecessor turns on stays on.
		for key, value := range previous {
			if value && !expanded[key] {
				t.Errorf("preset %s is not a superset of its predecessor: %q dropped", preset.ID, key)
			}
		}
		previous = expanded
	}
	if locked, _ := PresetEntitlements(PresetLocked); !reflect.DeepEqual(locked, allEntitlements(false)) {
		t.Fatalf("locked is not all off: %v", locked)
	}
	if full, _ := PresetEntitlements(PresetAgentComputer); !reflect.DeepEqual(full, allEntitlements(true)) {
		t.Fatalf("agent_computer is not every bit on: %v", full)
	}
	if _, ok := PresetEntitlements("nope"); ok {
		t.Fatal("an unknown preset expanded")
	}
	// OrgPolicyPresets is a copy.
	copyOf := OrgPolicyPresets()
	copyOf[1].Keys[0] = "mutated"
	if OrgPolicyPresets()[1].Keys[0] != "thread_create" {
		t.Fatal("OrgPolicyPresets exposed the live presets")
	}
}

func provisionedPolicy() EffectPolicy {
	return EffectPolicy{
		OrgID: "org-1", PrivacyPolicyRef: "privacy:org-1:1", Purpose: "assistant_collaboration",
		LawfulBasis: "contract", PrivacyClass: "internal", RetentionClass: "standard",
		Residency: "swedencentral", DeletionScope: "space",
	}
}

func TestBuildOrgPolicyViewNotProvisionedShape(t *testing.T) {
	view := BuildOrgPolicyView("org-1", nil, OrgPolicyRevisions{}, nil, 3, nil)
	if view.State != OrgPolicyStateNotProvisioned || view.OrgID != "org-1" {
		t.Fatalf("state = %q org = %q", view.State, view.OrgID)
	}
	if view.Attestation != nil || view.UpdatedAt != nil || view.MatchesPreset != nil || view.LastChange != nil {
		t.Fatalf("a not-provisioned view carries stored state: %+v", view)
	}
	if view.ActiveSpaces != 3 {
		t.Fatalf("active spaces = %d", view.ActiveSpaces)
	}
	if len(view.Entitlements) != len(EntitlementCatalog()) {
		t.Fatalf("entitlements = %d rows", len(view.Entitlements))
	}
	for _, row := range view.Entitlements {
		if row.Enabled || row.Effective || row.ZDRBlocked {
			t.Errorf("%s is on or blocked in a not-provisioned org: %+v", row.Key, row)
		}
		if len(row.Decisions) == 0 {
			t.Errorf("%s lists no decisions", row.Key)
		}
	}
	// Nothing is blocked, so every preset is on offer (the write itself fails
	// closed on a missing row).
	if len(view.Presets) != 5 {
		t.Fatalf("presets = %d", len(view.Presets))
	}
	for _, preset := range view.Presets {
		if !preset.Available || len(preset.BlockedBy) != 0 {
			t.Errorf("preset %s unavailable without ZDR: %+v", preset.ID, preset)
		}
	}
}

func TestBuildOrgPolicyViewEffectiveAndZDRBlocked(t *testing.T) {
	policy := provisionedPolicy()
	policy.ZeroDataRetention = true
	policy.ThreadCreateEntitled = true
	policy.ImportWriteEntitled = true       // inert under ZDR: enabled but not effective
	policy.SandboxCapabilityEntitled = true // unproven: still effective
	now := time.Date(2026, 10, 27, 12, 0, 0, 0, time.UTC)
	view := BuildOrgPolicyView("org-1", &policy, OrgPolicyRevisions{Policy: 2, Entitlement: 5}, &now, 1, nil)
	if view.State != OrgPolicyStateProvisioned || view.Attestation == nil || !view.Attestation.ZeroDataRetention {
		t.Fatalf("provisioned view = %+v", view)
	}
	if view.Revisions.Entitlement != 5 || view.Revisions.Policy != 2 || view.UpdatedAt == nil {
		t.Fatalf("revisions/updated_at = %+v %v", view.Revisions, view.UpdatedAt)
	}
	byKey := map[string]OrgPolicyEntitlementView{}
	for _, row := range view.Entitlements {
		byKey[row.Key] = row
	}
	if row := byKey["thread_create"]; !row.Enabled || !row.Effective || row.ZDRBlocked {
		t.Errorf("thread_create (compatible) under ZDR = %+v", row)
	}
	if row := byKey["import_write"]; !row.Enabled || row.Effective || !row.ZDRBlocked {
		t.Errorf("import_write (inert) under ZDR = %+v, want enabled, not effective, blocked", row)
	}
	if row := byKey["sandbox_capability"]; !row.Enabled || !row.Effective || !row.ZDRBlocked {
		t.Errorf("sandbox_capability (unproven) under ZDR = %+v, want enabled, effective, blocked from new enable", row)
	}
	if row := byKey["thread_read"]; row.Enabled || row.Effective {
		t.Errorf("thread_read off but reported on: %+v", row)
	}

	// Preset availability under ZDR: the ones naming only compatible bits stay on offer.
	avail := map[string]OrgPolicyPresetView{}
	for _, preset := range view.Presets {
		avail[preset.ID] = preset
	}
	for _, id := range []string{PresetLocked, PresetConversation, PresetConversationKnowledge} {
		if !avail[id].Available || len(avail[id].BlockedBy) != 0 {
			t.Errorf("preset %s should stay available under ZDR: %+v", id, avail[id])
		}
	}
	for _, id := range []string{PresetAgentWork, PresetAgentComputer} {
		if avail[id].Available || !reflect.DeepEqual(avail[id].BlockedBy, []string{"zero_data_retention"}) {
			t.Errorf("preset %s should be blocked by zero_data_retention: %+v", id, avail[id])
		}
	}
}

func TestBuildOrgPolicyViewMatchesPreset(t *testing.T) {
	check := func(name string, policy EffectPolicy, want string) {
		t.Helper()
		view := BuildOrgPolicyView("org-1", &policy, OrgPolicyRevisions{Policy: 1, Entitlement: 1}, nil, 0, nil)
		switch {
		case want == "" && view.MatchesPreset != nil:
			t.Errorf("%s: matched %q, want no preset", name, *view.MatchesPreset)
		case want != "" && (view.MatchesPreset == nil || *view.MatchesPreset != want):
			t.Errorf("%s: matches_preset = %v, want %q", name, view.MatchesPreset, want)
		}
	}
	check("all off is locked", provisionedPolicy(), PresetLocked)

	conv := provisionedPolicy()
	conv.ThreadCreateEntitled, conv.ThreadReadEntitled = true, true
	check("conversation", conv, PresetConversation)

	knowledge := conv
	knowledge.RetrievalReadEntitled = true
	check("conversation_knowledge", knowledge, PresetConversationKnowledge)

	full := provisionedPolicy()
	for _, spec := range EntitlementCatalog() {
		spec.Set(&full, true)
	}
	check("agent_computer", full, PresetAgentComputer)

	custom := provisionedPolicy()
	custom.ImportWriteEntitled = true
	check("custom", custom, "")
}

func TestOrgPolicyRequestDigestBindsOneExactRequest(t *testing.T) {
	base := ApplyOrgEntitlementsRequest{
		OrgID: "org-1", IdempotencyKey: "idem-0001", ExpectedEntitlementRevision: 3,
		Entitlements: withKeys("thread_create"),
	}
	if orgPolicyRequestDigest(base) != orgPolicyRequestDigest(base) {
		t.Fatal("digest is not deterministic")
	}
	// Org, actor and idempotency key are not digest inputs: the key scopes the
	// digest in the events table.
	other := base
	other.OrgID, other.IdempotencyKey = "org-2", "idem-0002"
	if orgPolicyRequestDigest(base) != orgPolicyRequestDigest(other) {
		t.Fatal("the digest must be over the request body only")
	}
	changed := base
	changed.Entitlements = withKeys("thread_create", "thread_read")
	revision := base
	revision.ExpectedEntitlementRevision = 4
	preset := ApplyOrgEntitlementsRequest{ExpectedEntitlementRevision: 3, PresetID: PresetConversation}
	digests := map[string]bool{}
	for _, req := range []ApplyOrgEntitlementsRequest{base, changed, revision, preset} {
		digests[orgPolicyRequestDigest(req)] = true
	}
	if len(digests) != 4 {
		t.Fatalf("distinct requests share a digest: %v", digests)
	}
	// Map iteration order must not matter.
	for i := 0; i < 20; i++ {
		if orgPolicyRequestDigest(changed) != orgPolicyRequestDigest(ApplyOrgEntitlementsRequest{
			ExpectedEntitlementRevision: 3, Entitlements: withKeys("thread_read", "thread_create"),
		}) {
			t.Fatal("digest depends on map order")
		}
	}
}

// ApplyOrgEntitlements validates the request before touching the transaction,
// so these run against a repository with a database handle but no pool.
func TestApplyOrgEntitlementsRefusesInvalidRequestsBeforeTheDatabase(t *testing.T) {
	repo := &Repository{db: &database.DB{}}
	actor := EffectPolicyActor{Type: "user", ID: "user-1", OrgRole: "admin"}
	valid := func() ApplyOrgEntitlementsRequest {
		return ApplyOrgEntitlementsRequest{
			OrgID: "org-1", Actor: actor, IdempotencyKey: "idem-0001", ExpectedEntitlementRevision: 1, PresetID: PresetConversation,
		}
	}
	type mutation struct {
		name  string
		edit  func(*ApplyOrgEntitlementsRequest)
		code  string
		field string
	}
	cases := []mutation{
		{"short idempotency key", func(r *ApplyOrgEntitlementsRequest) { r.IdempotencyKey = "short" }, OrgPolicyCodeInvalidRequest, "idempotency_key"},
		{"long idempotency key", func(r *ApplyOrgEntitlementsRequest) { r.IdempotencyKey = string(make([]byte, 129)) }, OrgPolicyCodeInvalidRequest, "idempotency_key"},
		{"revision missing", func(r *ApplyOrgEntitlementsRequest) { r.ExpectedEntitlementRevision = 0 }, OrgPolicyCodeInvalidRequest, "expected_entitlement_revision"},
		{"both preset and entitlements", func(r *ApplyOrgEntitlementsRequest) { r.Entitlements = allEntitlements(false) }, OrgPolicyCodeInvalidRequest, "preset_id"},
		{"neither preset nor entitlements", func(r *ApplyOrgEntitlementsRequest) { r.PresetID = "" }, OrgPolicyCodeInvalidRequest, "entitlements"},
		{"unknown preset", func(r *ApplyOrgEntitlementsRequest) { r.PresetID = "everything" }, OrgPolicyCodeUnknownPreset, ""},
	}
	for _, tc := range cases {
		req := valid()
		tc.edit(&req)
		_, err := repo.ApplyOrgEntitlements(nil, req) //nolint:staticcheck // validation precedes any use of ctx
		policyErr := asOrgPolicyError(t, err)
		if policyErr.Code != tc.code {
			t.Errorf("%s: code %q, want %q", tc.name, policyErr.Code, tc.code)
		}
		if tc.field != "" && policyErr.Details["field"] != tc.field {
			t.Errorf("%s: field %v, want %q", tc.name, policyErr.Details["field"], tc.field)
		}
	}
	// The org and the actor are required.
	noOrg := valid()
	noOrg.OrgID = "  "
	if _, err := repo.ApplyOrgEntitlements(nil, noOrg); err == nil { //nolint:staticcheck
		t.Error("a blank org was accepted")
	}
	noActor := valid()
	noActor.Actor = EffectPolicyActor{}
	if _, err := repo.ApplyOrgEntitlements(nil, noActor); err == nil { //nolint:staticcheck
		t.Error("an actor-less write was accepted")
	}
	// An unwired repository fails closed.
	if _, err := (*Repository)(nil).ApplyOrgEntitlements(nil, valid()); err == nil { //nolint:staticcheck
		t.Error("a nil repository accepted a write")
	}
}

func TestDiffEntitlementMapsTreatsMissingKeysAsFalseInCatalogueOrder(t *testing.T) {
	before := map[string]bool{"thread_create": true}
	after := map[string]bool{"thread_create": true, "retrieval_read": true, "thread_read": true}
	got := diffEntitlementMaps(before, after)
	var keys []string
	for _, change := range got {
		keys = append(keys, change.Key)
		if change.From || !change.To {
			t.Errorf("change %+v", change)
		}
	}
	if !reflect.DeepEqual(keys, []string{"thread_read", "retrieval_read"}) {
		t.Fatalf("diff keys = %v, want catalogue order", keys)
	}
	if empty := diffEntitlementMaps(nil, nil); empty == nil || len(empty) != 0 {
		t.Fatalf("an empty diff must be a non-nil empty slice (it is rendered as JSON []): %#v", empty)
	}
}

func TestOrgPolicyErrorRendersItsMessage(t *testing.T) {
	err := orgPolicyErr(OrgPolicyCodeRevisionConflict, "entitlements changed since they were read", map[string]any{"current_entitlement_revision": int64(9)})
	if err.Error() != "entitlements changed since they were read" || err.Code != "space_policy_revision_conflict" {
		t.Fatalf("OrgPolicyError = %+v", err)
	}
}
