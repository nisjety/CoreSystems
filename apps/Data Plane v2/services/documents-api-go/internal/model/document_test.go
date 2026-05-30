package model

import "testing"

func TestIngestPolicy_IsZeroRetention(t *testing.T) {
	cases := []struct {
		name   string
		policy *IngestPolicy
		want   bool
	}{
		{"nil policy is non-ZDR", nil, false},
		{"default off-mode is non-ZDR", &IngestPolicy{ZDRMode: "off"}, false},
		{"explicit on mode is ZDR", &IngestPolicy{ZDRMode: "on"}, true},
		{"ephemeral_only flag is ZDR", &IngestPolicy{EphemeralOnly: true}, true},
		{"both flags is ZDR", &IngestPolicy{ZDRMode: "on", EphemeralOnly: true}, true},
		{"empty struct is non-ZDR", &IngestPolicy{}, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := tc.policy.IsZeroRetention(); got != tc.want {
				t.Errorf("IsZeroRetention() = %v, want %v", got, tc.want)
			}
		})
	}
}
