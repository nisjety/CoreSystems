package spaces

// OrgPolicyPreset is a named, cumulative set of entitlement bits an org admin
// can apply in one step (WS03-S3). Presets only ever name entitlement keys from
// the catalogue; they never touch the privacy/attestation fields.
type OrgPolicyPreset struct {
	// ID is the stable public name of the preset.
	ID string
	// Keys are the entitlement keys the preset turns ON; every other
	// catalogue key is turned OFF.
	Keys []string
}

// Preset ids (cumulative, in this order).
const (
	PresetLocked                = "locked"
	PresetConversation          = "conversation"
	PresetConversationKnowledge = "conversation_knowledge"
	PresetAgentWork             = "agent_work"
	PresetAgentComputer         = "agent_computer"
)

// orgPolicyPresets lists the presets in fixed order. Each one adds to the one
// before it (locked < conversation < conversation_knowledge < agent_work <
// agent_computer), so a preset is always a superset of its predecessor.
var orgPolicyPresets = []OrgPolicyPreset{
	{ID: PresetLocked, Keys: nil},
	{ID: PresetConversation, Keys: []string{"thread_create", "thread_read"}},
	{ID: PresetConversationKnowledge, Keys: []string{"thread_create", "thread_read", "retrieval_read"}},
	{ID: PresetAgentWork, Keys: []string{
		"thread_create", "thread_read", "retrieval_read", "import_write", "agent_action", "schedule_fire",
	}},
	{ID: PresetAgentComputer, Keys: []string{
		"thread_create", "thread_read", "retrieval_read", "import_write", "agent_action", "schedule_fire",
		"sandbox_capability", "process_registry",
	}},
}

// OrgPolicyPresets returns a copy of the presets in fixed order.
func OrgPolicyPresets() []OrgPolicyPreset {
	out := make([]OrgPolicyPreset, len(orgPolicyPresets))
	for i, preset := range orgPolicyPresets {
		out[i] = OrgPolicyPreset{ID: preset.ID, Keys: append([]string(nil), preset.Keys...)}
	}
	return out
}

// PresetEntitlements expands a preset into the full desired state: every
// catalogue key, true for the preset's keys and false for the rest.
func PresetEntitlements(id string) (map[string]bool, bool) {
	for _, preset := range orgPolicyPresets {
		if preset.ID != id {
			continue
		}
		out := make(map[string]bool, len(entitlementCatalog))
		for _, spec := range entitlementCatalog {
			out[spec.Key] = false
		}
		for _, key := range preset.Keys {
			out[key] = true
		}
		return out, true
	}
	return nil, false
}
