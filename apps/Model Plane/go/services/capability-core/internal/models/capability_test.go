package models

import "testing"

// TestIsSeededHighRiskCapability locks down the exact set of migration-seeded
// RiskHigh capability ids that capabilities_store.go's Upsert treats as
// protected even when their current row cannot be read (POL-1's fail-closed
// backstop). Any future migration that seeds a new RiskHigh capability must
// update seededHighRiskCapabilityIDs and this test together.
func TestIsSeededHighRiskCapability(t *testing.T) {
	t.Parallel()

	tests := []struct {
		id   string
		want bool
	}{
		{id: "cap.command.shell", want: true},
		{id: "cap.browser.open", want: true},
		{id: "cap.tool.shipping.book", want: true},
		{id: "cap.tool.social.publish", want: true},
		{id: "cap.tool.provider.execute", want: true},
		{id: "cap.self_owned.misp_opencti", want: true},
		{id: "cap.self_owned.quarry_url_reputation_feeds", want: true},
		{id: "cap.self_owned.opensanctions_yente", want: true},
		{id: "", want: false},
		{id: "cap.command.sandbox", want: false},     // deliberately seeded low, see migration 0010
		{id: "cap.tool.shipping.track", want: false}, // seeded low
		{id: "cap.tenant.custom-capability", want: false},
	}

	for _, test := range tests {
		t.Run(test.id, func(t *testing.T) {
			t.Parallel()
			if got := IsSeededHighRiskCapability(test.id); got != test.want {
				t.Fatalf("IsSeededHighRiskCapability(%q) = %v, want %v", test.id, got, test.want)
			}
		})
	}
}

func TestIsSupportedRiskLevel(t *testing.T) {
	t.Parallel()

	tests := []struct {
		level string
		want  bool
	}{
		{level: RiskLow, want: true},
		{level: RiskMedium, want: true},
		{level: RiskHigh, want: true},
		{level: "", want: false},
		{level: "critical", want: false},
		{level: "LOW", want: false},
	}

	for _, test := range tests {
		t.Run(test.level, func(t *testing.T) {
			t.Parallel()
			if got := IsSupportedRiskLevel(test.level); got != test.want {
				t.Fatalf("IsSupportedRiskLevel(%q) = %v, want %v", test.level, got, test.want)
			}
		})
	}
}
