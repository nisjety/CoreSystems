package activities

import "testing"

// The scheduled Do lane is on only for the exact literal "true", as execution-core reads the same
// variable. A padded or cased value, or any other word, leaves the lane off.
func TestScheduledAgentLaneSwitchIsTheExactLiteral(t *testing.T) {
	for _, tc := range []struct {
		value string
		on    bool
	}{
		{"true", true},
		{"TRUE", false},
		{"True", false},
		{" true", false},
		{"true ", false},
		{"1", false},
		{"yes", false},
		{"", false},
	} {
		t.Setenv("EXECUTION_SCHEDULED_AGENT_ENABLED", tc.value)
		if got := scheduledAgentLaneEnabled(); got != tc.on {
			t.Errorf("EXECUTION_SCHEDULED_AGENT_ENABLED=%q: scheduledAgentLaneEnabled() = %v, want %v",
				tc.value, got, tc.on)
		}
	}
}
