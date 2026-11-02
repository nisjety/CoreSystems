package spaces

import (
	"reflect"
	"strings"
	"testing"
	"time"
)

// WS03-S1: the single entitlement catalogue. Adding an entitlement bit is one
// catalogue row plus one migration; these tests are what stop a bit being added
// to EffectPolicy without its row (or a row whose ZDR class lies about what
// the real Issue* function does).

// entitledJSONFields returns every json "*_entitled" tag of EffectPolicy
// mapped to its struct field name.
func entitledJSONFields(t *testing.T) map[string]string {
	t.Helper()
	fields := map[string]string{}
	typ := reflect.TypeOf(EffectPolicy{})
	for i := 0; i < typ.NumField(); i++ {
		tag := strings.Split(typ.Field(i).Tag.Get("json"), ",")[0]
		if strings.HasSuffix(tag, "_entitled") {
			fields[tag] = typ.Field(i).Name
		}
	}
	return fields
}

func TestEveryEntitledFieldOfEffectPolicyHasACatalogueRow(t *testing.T) {
	fields := entitledJSONFields(t)
	if len(fields) == 0 {
		t.Fatal("found no *_entitled fields on EffectPolicy; the reflection guard is broken")
	}
	columns := map[string]EntitlementSpec{}
	for _, spec := range EntitlementCatalog() {
		columns[spec.Column] = spec
	}
	for tag, field := range fields {
		spec, ok := columns[tag]
		if !ok {
			t.Errorf("EffectPolicy.%s (json %q) has no entitlement catalogue row", field, tag)
			continue
		}
		// The row's Get/Set must address that very field.
		var probe EffectPolicy
		spec.Set(&probe, true)
		if !reflect.ValueOf(probe).FieldByName(field).Bool() {
			t.Errorf("catalogue row %q Set does not write EffectPolicy.%s", spec.Key, field)
		}
		if !spec.Get(probe) {
			t.Errorf("catalogue row %q Get does not read EffectPolicy.%s", spec.Key, field)
		}
	}
	for column := range columns {
		if _, ok := fields[column]; !ok {
			t.Errorf("catalogue column %q has no *_entitled field on EffectPolicy", column)
		}
	}
	if len(columns) != len(EntitlementCatalog()) {
		t.Error("catalogue columns are not unique")
	}
}

func TestEntitlementCatalogueIsWellFormedAndOrdered(t *testing.T) {
	wantOrder := []string{
		"thread_create", "thread_read", "retrieval_read", "import_write",
		"agent_action", "schedule_fire", "sandbox_capability", "process_registry",
		// W2 rows, appended in build order.
		"run_control", "approval_grant", "dm_create", "message_revise", "message_moderate",
	}
	var gotOrder []string
	seenKeys := map[string]bool{}
	for _, spec := range EntitlementCatalog() {
		gotOrder = append(gotOrder, spec.Key)
		if seenKeys[spec.Key] {
			t.Errorf("duplicate key %q", spec.Key)
		}
		seenKeys[spec.Key] = true
		if spec.Column != spec.Key+"_entitled" {
			t.Errorf("key %q column %q: column must be key + _entitled", spec.Key, spec.Column)
		}
		if len(spec.Decisions) == 0 {
			t.Errorf("key %q gates no decision type", spec.Key)
		}
		switch spec.ZDR {
		case ZDRCompatible, ZDRInert, ZDRUnproven:
		default:
			t.Errorf("key %q has unknown ZDR class %q", spec.Key, spec.ZDR)
		}
		if spec.Get == nil || spec.Set == nil {
			t.Errorf("key %q lacks Get/Set", spec.Key)
		}
		if got, ok := EntitlementSpecByKey(spec.Key); !ok || got.Column != spec.Column {
			t.Errorf("EntitlementSpecByKey(%q) = %v, %v", spec.Key, got, ok)
		}
	}
	// Order is part of the contract: views, presets and audit details render in it.
	if !reflect.DeepEqual(gotOrder, wantOrder) {
		t.Fatalf("catalogue order changed:\n got %v\nwant %v", gotOrder, wantOrder)
	}
	if _, ok := EntitlementSpecByKey("nope"); ok {
		t.Error("EntitlementSpecByKey found a key that does not exist")
	}
	// The returned slice is a copy.
	catalogue := EntitlementCatalog()
	catalogue[0].Key = "mutated"
	if EntitlementCatalog()[0].Key != "thread_create" {
		t.Error("EntitlementCatalog exposed the live catalogue")
	}
}

func TestEntitlementValuesDiffAndColumnLists(t *testing.T) {
	var none EffectPolicy
	values := EntitlementValues(none)
	if len(values) != len(EntitlementCatalog()) {
		t.Fatalf("EntitlementValues rendered %d keys, want %d", len(values), len(EntitlementCatalog()))
	}
	for key, value := range values {
		if value {
			t.Errorf("zero policy has %q on", key)
		}
	}

	next := none
	next.ThreadReadEntitled = true
	next.ProcessRegistryEntitled = true
	changes := DiffEntitlements(none, next)
	want := []EntitlementChange{{Key: "thread_read", From: false, To: true}, {Key: "process_registry", From: false, To: true}}
	if !reflect.DeepEqual(changes, want) {
		t.Fatalf("DiffEntitlements = %v, want %v (catalogue order)", changes, want)
	}
	if !entitlementsEqual(next, next) || entitlementsEqual(none, next) {
		t.Fatal("entitlementsEqual wrong")
	}
	if got := DiffEntitlements(next, none); got[0].From != true || got[0].To != false {
		t.Fatalf("reverse diff = %v", got)
	}

	list := entitlementColumnList()
	if !strings.HasPrefix(list, "thread_create_entitled, ") || !strings.HasSuffix(list, ", message_moderate_entitled") {
		t.Fatalf("entitlementColumnList = %q", list)
	}
	full := effectPolicyColumnList()
	if !strings.HasPrefix(full, "org_id, privacy_policy_ref, ") || !strings.HasSuffix(full, list) {
		t.Fatalf("effectPolicyColumnList = %q", full)
	}
	if strings.Count(full, ",")+1 != 1+len(effectPolicyPrivacyColumns)+len(EntitlementCatalog()) {
		t.Fatalf("effectPolicyColumnList has the wrong column count: %q", full)
	}
	if got := len(policyPrivacyValues(EffectPolicy{})); got != len(effectPolicyPrivacyColumns) {
		t.Fatalf("policyPrivacyValues returns %d values for %d privacy columns", got, len(effectPolicyPrivacyColumns))
	}
}

// zdrProbe exercises the REAL Issue* function a catalogue key gates, under a
// Zero Data Retention posture, and reports (issued, zdrDenied, decision).
type zdrProbe func(t *testing.T) (issued bool, err error, decision Decision)

func zdrProbes() map[string]zdrProbe {
	now := time.Date(2026, 10, 27, 12, 0, 0, 0, time.UTC)
	return map[string]zdrProbe{
		"thread_create": func(t *testing.T) (bool, error, Decision) {
			evidence := validPersonalThreadEvidence()
			evidence.Privacy.ZeroDataRetention = true
			decision, err := IssuePersonalThreadCreateDecision(evidence, validPersonalThreadRequest(), now)
			return err == nil, err, decision
		},
		"thread_read": func(t *testing.T) (bool, error, Decision) {
			evidence := validSharedThreadReadEvidence()
			evidence.Privacy.ZeroDataRetention = true
			decision, err := IssueSharedThreadReadDecision(evidence, SharedThreadReadDecisionRequest{
				DecisionRef: "d", IdempotencyKey: "i", Nonce: "n",
			}, now)
			return err == nil, err, decision
		},
		"retrieval_read": func(t *testing.T) (bool, error, Decision) {
			evidence := validPersonalRetrievalEvidence()
			evidence.Privacy.ZeroDataRetention = true
			decision, err := IssuePersonalRetrievalDecision(evidence, PersonalRetrievalDecisionRequest{
				DecisionRef: "d", IdempotencyKey: "i", Nonce: "n",
			}, now)
			return err == nil, err, decision
		},
		"import_write": func(t *testing.T) (bool, error, Decision) {
			evidence := validPersonalImportEvidence()
			evidence.Privacy.ZeroDataRetention = true
			decision, err := IssuePersonalImportDecision(evidence, PersonalImportDecisionRequest{
				DecisionRef: "d", IdempotencyKey: "i", SourceType: "notion", Nonce: "n",
			}, now)
			return err == nil, err, decision
		},
		"agent_action": func(t *testing.T) (bool, error, Decision) {
			evidence := validPersonalThreadEvidence()
			evidence.AgentActionEntitled = true
			evidence.Membership.Role = "owner"
			evidence.Privacy.ZeroDataRetention = true
			_, err := IssueOwnerGrantDecision(evidence, validOwnerGrantRequest(), now)
			return err == nil, err, Decision{}
		},
		"schedule_fire": func(t *testing.T) (bool, error, Decision) {
			evidence := validPersonalThreadEvidence()
			evidence.ScheduleFireEntitled = true
			evidence.Privacy.ZeroDataRetention = true
			decision, err := IssueScheduleCreateDecision(evidence, validScheduleCreateRequest(), now)
			return err == nil, err, decision
		},
		"sandbox_capability": func(t *testing.T) (bool, error, Decision) {
			evidence := validPersonalThreadEvidence()
			evidence.SandboxCapabilityEntitled = true
			evidence.Privacy.ZeroDataRetention = true
			decision, err := IssueSpaceCapabilityDecision(evidence, validSpaceCapabilityIntent(), "d", "n", now)
			return err == nil, err, decision
		},
		"process_registry": func(t *testing.T) (bool, error, Decision) {
			evidence := validPersonalThreadEvidence()
			evidence.SandboxCapabilityEntitled = true
			evidence.ProcessRegistryEntitled = true
			evidence.Privacy.ZeroDataRetention = true
			intent := validSpaceCapabilityIntent()
			intent.Processes = ProcessesBackgroundRegistry
			decision, err := IssueSpaceCapabilityDecision(evidence, intent, "d", "n", now)
			return err == nil, err, decision
		},
		// WS09-S7: cancelling a run records nothing durable here; the decision
		// is issued under ZDR and carries the claim.
		"run_control": func(t *testing.T) (bool, error, Decision) {
			evidence := validSharedThreadReadEvidence()
			evidence.Membership.Role = "manager"
			evidence.ThreadReadEntitled = false
			evidence.RunControlEntitled = true
			evidence.ResourceAuthorizationRef = "control:room-1:run-control:7"
			evidence.Privacy.ZeroDataRetention = true
			decision, err := IssueSharedRunControlDecision(evidence, RunControlDecisionRequest{
				DecisionRef: "d", RunID: "run-1", Operation: RunControlOperationCancel, IdempotencyKey: "i", Nonce: "n",
			}, now)
			return err == nil, err, decision
		},
		// WS10: a durable consent record fails closed under ZDR.
		"approval_grant": func(t *testing.T) (bool, error, Decision) {
			evidence := validSharedThreadReadEvidence()
			evidence.Membership.Role = "manager"
			evidence.ApprovalGrantEntitled = true
			evidence.Privacy.ZeroDataRetention = true
			_, err := IssueApprovalDecideDecision(evidence, ApprovalDecideAuthority{}, ApprovalDecideRequest{}, now)
			return err == nil, err, Decision{}
		},
		// WS12-S5: revising or moderating rewrites stored content.
		"message_revise": func(t *testing.T) (bool, error, Decision) {
			evidence := validSharedThreadReadEvidence()
			evidence.Membership.Role = "editor"
			evidence.MessageReviseEntitled = true
			evidence.OriginalRetention = OriginalRetentionRetain
			evidence.Privacy.ZeroDataRetention = true
			decision, err := IssueThreadReviseDecision(evidence, ThreadReviseDecisionRequest{}, now)
			return err == nil, err, decision
		},
		"message_moderate": func(t *testing.T) (bool, error, Decision) {
			evidence := validSharedThreadReadEvidence()
			evidence.Membership.Role = "manager"
			evidence.MessageReviseEntitled = true
			evidence.MessageModerateEntitled = true
			evidence.OriginalRetention = OriginalRetentionRetain
			evidence.Privacy.ZeroDataRetention = true
			decision, err := IssueThreadReviseDecision(evidence, ThreadReviseDecisionRequest{}, now)
			return err == nil, err, decision
		},
	}
}

// zdrDBOnlyEntitlements are catalogue keys whose ZDR refusal is a database
// check, not a pure Issue function, so no in-process probe exists. dm_create:
// requireDMEntitlements reads zero_data_retention from the policy row and
// reports it as DMNotEntitledError{Missing: [zero_data_retention]}.
var zdrDBOnlyEntitlements = map[string]string{"dm_create": "requireDMEntitlements (DB)"}

func TestEntitlementZDRClassIsBoundToTheRealIssueFunctions(t *testing.T) {
	probes := zdrProbes()
	for _, spec := range EntitlementCatalog() {
		if _, dbOnly := zdrDBOnlyEntitlements[spec.Key]; dbOnly {
			if spec.ZDR != ZDRInert {
				t.Errorf("%s ZDR refusal lives in the DM eligibility query and must be classed zdr_inert", spec.Key)
			}
			continue
		}
		probe, ok := probes[spec.Key]
		if !ok {
			t.Errorf("catalogue key %q has no ZDR probe; add one when adding an entitlement", spec.Key)
			continue
		}
		t.Run(spec.Key, func(t *testing.T) {
			issued, err, decision := probe(t)
			switch spec.ZDR {
			case ZDRInert:
				// Control refuses ZDR evidence for this bit, so enabling it would be inert.
				if issued {
					t.Fatalf("%s is classed zdr_inert but its Issue function issued under ZDR", spec.Key)
				}
				denial, isDenial := AsDenial(err)
				if !isDenial || denial.Code != DenialZDRForbidsEffect {
					t.Fatalf("%s zdr_inert refusal = %v, want a space_zdr_forbids_effect denial", spec.Key, err)
				}
			case ZDRCompatible:
				if !issued {
					t.Fatalf("%s is classed compatible but its Issue function refused under ZDR: %v", spec.Key, err)
				}
				if !decision.ZeroDataRetention {
					t.Fatalf("%s decision under ZDR does not carry the zdr claim", spec.Key)
				}
			case ZDRUnproven:
				// Control does not refuse; the downstream plane has no proven ZDR
				// handling, so phase-1 writes refuse it for a ZDR org instead.
				if !issued {
					t.Fatalf("%s is classed zdr_unproven so Control must not refuse it: %v", spec.Key, err)
				}
			}
		})
	}
	for key := range probes {
		if _, ok := EntitlementSpecByKey(key); !ok {
			t.Errorf("probe %q has no catalogue row", key)
		}
	}
}

func TestProcessRegistryPermissionIsWithheldUnderZDR(t *testing.T) {
	issued, err, decision := zdrProbes()["process_registry"](t)
	if !issued {
		t.Fatalf("process registry decision refused under ZDR: %v", err)
	}
	for _, permission := range decision.Permissions {
		if permission == "space:processes" {
			t.Fatalf("space:processes was granted under ZDR: %v", decision.Permissions)
		}
	}
}
