package config

import "testing"

func TestValidateScopedServiceTokensFailsClosed(t *testing.T) {
	valid := &Config{
		OrgCore:     OrgCoreConfig{ServiceToken: "session-org-token-at-least-32-bytes"},
		BillingCore: BillingCoreConfig{ServiceToken: "session-billing-token-at-least-32-bytes"},
		UserCore:    UserCoreConfig{ServiceToken: "session-user-token-at-least-32-bytes"},
	}
	if err := valid.ValidateScopedServiceTokens(); err != nil {
		t.Fatalf("valid scoped tokens rejected: %v", err)
	}

	for _, test := range []struct {
		name  string
		apply func(*Config)
	}{
		{name: "missing org", apply: func(c *Config) { c.OrgCore.ServiceToken = "" }},
		{name: "test org", apply: func(c *Config) { c.OrgCore.ServiceToken = "test-org-service-token-at-least-32-bytes" }},
		{name: "placeholder org", apply: func(c *Config) { c.OrgCore.ServiceToken = "placeholder-org-service-token-at-least-32-bytes" }},
		{name: "change-me billing", apply: func(c *Config) { c.BillingCore.ServiceToken = "change-me-billing-service-token-1234" }},
		{name: "replace-with billing", apply: func(c *Config) { c.BillingCore.ServiceToken = "replace-with-billing-service-token-1234" }},
		{name: "missing user", apply: func(c *Config) { c.UserCore.ServiceToken = "" }},
		{name: "test user", apply: func(c *Config) { c.UserCore.ServiceToken = "test-user-service-token-at-least-32-bytes" }},
		{name: "duplicate audience tokens", apply: func(c *Config) { c.BillingCore.ServiceToken = c.OrgCore.ServiceToken }},
	} {
		t.Run(test.name, func(t *testing.T) {
			candidate := *valid
			test.apply(&candidate)
			if err := candidate.ValidateScopedServiceTokens(); err == nil {
				t.Fatal("unsafe scoped token was accepted")
			}
		})
	}
}
