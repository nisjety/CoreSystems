package spaces

import (
	"strings"

	"github.com/jackc/pgx/v5"
)

// ZDRClass says how an entitlement behaves for an organisation whose Space
// privacy attestation declares Zero Data Retention.
type ZDRClass string

const (
	// ZDRCompatible: Control issues the decision under Space ZDR (with the zdr
	// claim); the bit may be enabled.
	ZDRCompatible ZDRClass = "compatible"
	// ZDRInert: Control refuses the decision under Space ZDR, so enabling the
	// bit would be inert. Phase-1 writes refuse it for a ZDR org.
	ZDRInert ZDRClass = "zdr_inert"
	// ZDRUnproven: Control does not refuse it, but the downstream plane has no
	// proven ZDR handling yet, so phase-1 writes refuse it for a ZDR org until
	// that is shown (decision D3).
	ZDRUnproven ZDRClass = "zdr_unproven"
)

// EntitlementSpec is one row of the single entitlement catalogue. The Space
// effect-policy view, validator, diff, presets, audit details and the SQL
// column lists are all generated from it, so adding an entitlement bit is one
// row here plus one migration; no other SQL has to change.
type EntitlementSpec struct {
	// Key is the stable, public name of the bit (never reused).
	Key string
	// Column is the space_effect_policies column holding the bit.
	Column string
	// Decisions lists the Control decision types this bit gates.
	Decisions []string
	// ZDR classifies the bit under Space Zero Data Retention.
	ZDR ZDRClass
	// Get reads the bit from a policy; Set writes it.
	Get func(EffectPolicy) bool
	Set func(*EffectPolicy, bool)
}

// entitlementCatalog lists every per-org entitlement boolean of
// space_effect_policies in fixed order. Order is part of the contract (views,
// presets and audit details render in it).
var entitlementCatalog = []EntitlementSpec{
	{
		Key: "thread_create", Column: "thread_create_entitled",
		Decisions: []string{"model.thread.create", "model.thread.append", "model.run.wake"}, ZDR: ZDRCompatible,
		Get: func(p EffectPolicy) bool { return p.ThreadCreateEntitled },
		Set: func(p *EffectPolicy, v bool) { p.ThreadCreateEntitled = v },
	},
	{
		Key: "thread_read", Column: "thread_read_entitled",
		Decisions: []string{"model.thread.read", "model.watch.create", "model.watch.observe"}, ZDR: ZDRCompatible,
		Get: func(p EffectPolicy) bool { return p.ThreadReadEntitled },
		Set: func(p *EffectPolicy, v bool) { p.ThreadReadEntitled = v },
	},
	{
		Key: "retrieval_read", Column: "retrieval_read_entitled",
		Decisions: []string{"data.retrieval.read"}, ZDR: ZDRCompatible,
		Get: func(p EffectPolicy) bool { return p.RetrievalReadEntitled },
		Set: func(p *EffectPolicy, v bool) { p.RetrievalReadEntitled = v },
	},
	{
		Key: "import_write", Column: "import_write_entitled",
		Decisions: []string{"ingestion.import.write"}, ZDR: ZDRInert,
		Get: func(p EffectPolicy) bool { return p.ImportWriteEntitled },
		Set: func(p *EffectPolicy, v bool) { p.ImportWriteEntitled = v },
	},
	{
		Key: "agent_action", Column: "agent_action_entitled",
		Decisions: []string{"model.owner_grant", "model.run.action", "model.schedule.step"}, ZDR: ZDRInert,
		Get: func(p EffectPolicy) bool { return p.AgentActionEntitled },
		Set: func(p *EffectPolicy, v bool) { p.AgentActionEntitled = v },
	},
	{
		Key: "schedule_fire", Column: "schedule_fire_entitled",
		Decisions: []string{"model.cron.create", "model.cron.fire", "model.schedule.run", "model.schedule.execute", "model.schedule.step"}, ZDR: ZDRInert,
		Get: func(p EffectPolicy) bool { return p.ScheduleFireEntitled },
		Set: func(p *EffectPolicy, v bool) { p.ScheduleFireEntitled = v },
	},
	{
		Key: "sandbox_capability", Column: "sandbox_capability_entitled",
		Decisions: []string{"model.space.capability_profile"}, ZDR: ZDRUnproven,
		Get: func(p EffectPolicy) bool { return p.SandboxCapabilityEntitled },
		Set: func(p *EffectPolicy, v bool) { p.SandboxCapabilityEntitled = v },
	},
	{
		Key: "process_registry", Column: "process_registry_entitled",
		Decisions: []string{"model.space.capability_profile#space:processes"}, ZDR: ZDRUnproven,
		Get: func(p EffectPolicy) bool { return p.ProcessRegistryEntitled },
		Set: func(p *EffectPolicy, v bool) { p.ProcessRegistryEntitled = v },
	},
	{
		Key: "run_control", Column: "run_control_entitled",
		Decisions: []string{"model.run.control"}, ZDR: ZDRCompatible,
		Get: func(p EffectPolicy) bool { return p.RunControlEntitled },
		Set: func(p *EffectPolicy, v bool) { p.RunControlEntitled = v },
	},
}

// effectPolicyPrivacyColumns are the nine privacy/attestation columns, in the
// order policyPrivacyValues returns them.
var effectPolicyPrivacyColumns = []string{
	"privacy_policy_ref", "purpose", "lawful_basis", "privacy_class",
	"third_party_processing_allowed", "retention_class", "residency",
	"deletion_scope", "zero_data_retention",
}

func policyPrivacyValues(p EffectPolicy) []any {
	return []any{
		p.PrivacyPolicyRef, p.Purpose, p.LawfulBasis, p.PrivacyClass,
		p.ThirdPartyProcessingAllowed, p.RetentionClass, p.Residency,
		p.DeletionScope, p.ZeroDataRetention,
	}
}

// EntitlementCatalog returns a copy of the catalogue in fixed order.
func EntitlementCatalog() []EntitlementSpec {
	out := make([]EntitlementSpec, len(entitlementCatalog))
	copy(out, entitlementCatalog)
	return out
}

// EntitlementSpecByKey looks one catalogue row up by its public key.
func EntitlementSpecByKey(key string) (EntitlementSpec, bool) {
	for _, spec := range entitlementCatalog {
		if spec.Key == key {
			return spec, true
		}
	}
	return EntitlementSpec{}, false
}

// EntitlementValues renders a policy's entitlement bits as key -> value.
func EntitlementValues(p EffectPolicy) map[string]bool {
	out := make(map[string]bool, len(entitlementCatalog))
	for _, spec := range entitlementCatalog {
		out[spec.Key] = spec.Get(p)
	}
	return out
}

// EntitlementChange is one entitlement bit that differs between two policies.
type EntitlementChange struct {
	Key  string `json:"key"`
	From bool   `json:"from"`
	To   bool   `json:"to"`
}

// DiffEntitlements lists the bits that differ, in catalogue order.
func DiffEntitlements(before, after EffectPolicy) []EntitlementChange {
	var changes []EntitlementChange
	for _, spec := range entitlementCatalog {
		from, to := spec.Get(before), spec.Get(after)
		if from != to {
			changes = append(changes, EntitlementChange{Key: spec.Key, From: from, To: to})
		}
	}
	return changes
}

// entitlementsEqual reports whether every catalogue bit matches.
func entitlementsEqual(a, b EffectPolicy) bool {
	return len(DiffEntitlements(a, b)) == 0
}

// entitlementColumnList renders "col_a, col_b, ..." for SELECT lists.
func entitlementColumnList() string {
	cols := make([]string, len(entitlementCatalog))
	for i, spec := range entitlementCatalog {
		cols[i] = spec.Column
	}
	return strings.Join(cols, ", ")
}

// effectPolicyColumnList is the full SELECT list: org_id, the privacy
// columns, then every catalogue column.
func effectPolicyColumnList() string {
	return "org_id, " + strings.Join(effectPolicyPrivacyColumns, ", ") + ", " + entitlementColumnList()
}

// scanEffectPolicy scans one row selected with effectPolicyColumnList into p;
// extra destinations receive any columns selected after the list.
func scanEffectPolicy(row pgx.Row, p *EffectPolicy, extra ...any) error {
	bits := make([]bool, len(entitlementCatalog))
	dest := []any{
		&p.OrgID, &p.PrivacyPolicyRef, &p.Purpose, &p.LawfulBasis, &p.PrivacyClass,
		&p.ThirdPartyProcessingAllowed, &p.RetentionClass, &p.Residency,
		&p.DeletionScope, &p.ZeroDataRetention,
	}
	for i := range bits {
		dest = append(dest, &bits[i])
	}
	dest = append(dest, extra...)
	if err := row.Scan(dest...); err != nil {
		return err
	}
	for i, spec := range entitlementCatalog {
		spec.Set(p, bits[i])
	}
	return nil
}
