package api

import "testing"

// Plan 3.4: every switch capability-core reads through a FromLookup parser turns on only for the
// literal "true", and reads exactly the variable it is named for.
func TestCapabilityCoreSwitchParsersAcceptOnlyTheLiteralTrue(t *testing.T) {
	parsers := map[string]func(func(string) string) bool{
		"SKILL_ORG_OWNER_CHECK":               SkillOrgOwnerCheckFromLookup,
		"MEMORY_GATE_CAPABILITY_WRITES":       MemoryGateCapabilityWritesFromLookup,
		"CAPABILITY_PERMISSION_RULES_ENABLED": PermissionRulesEnabledFromLookup,
		"PLUGIN_LOADER_ENABLED":               PluginLoaderFromLookup,
	}
	values := map[string]bool{"true": true, "": false, "TRUE": false, "True": false, "1": false, "yes": false, "on": false, " true": false, "true\n": false, "false": false}
	for name, parse := range parsers {
		for value, want := range values {
			var read []string
			got := parse(func(key string) string {
				read = append(read, key)
				if key == name {
					return value
				}
				return "true"
			})
			if got != want {
				t.Errorf("%s=%q -> %t, want %t", name, value, got, want)
			}
			if len(read) != 1 || read[0] != name {
				t.Errorf("%s parser read %v", name, read)
			}
		}
	}
}
