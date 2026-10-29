package api

import "testing"

func TestAnalyticsActionsRequireAnalyticsReadCapability(t *testing.T) {
	for _, operation := range []string{"ga4.account_summaries.list", "ga4.data_streams.list", "ga4.report.run", "ga4.reporting_host.resolve"} {
		capability, sensitive := requiredCapabilityForOperation("google", operation)
		if capability != "analytics.read" || !sensitive || actionRequiresApproval("google", operation) {
			t.Fatalf("operation=%s capability=%s sensitive=%v", operation, capability, sensitive)
		}
	}
}
