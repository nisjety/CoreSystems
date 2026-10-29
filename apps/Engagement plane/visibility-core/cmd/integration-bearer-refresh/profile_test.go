package main

import "testing"

func TestStudioBearerAliasesOverrideOnlyWhenPresent(t *testing.T) {
	for _, test := range []struct {
		modern, legacy, value string
		read                  func() string
	}{
		{"STUDIO_SERVICE_ID", "MARKETING_INTEGRATION_SERVICE_ID", "studio-quarry", func() string { return configFromEnvironment().ServiceID }},
		{"STUDIO_ORGANIZATION_ID", "MARKETING_INTEGRATION_ORGANIZATION_ID", "studio-org", func() string { return configFromEnvironment().OrganizationID }},
		{"STUDIO_SERVICE_KEY_FILE", "MARKETING_INTEGRATION_SERVICE_KEY_FILE", "/run/secrets/studio", func() string { return configFromEnvironment().ServiceKeyFile }},
		{"STUDIO_BEARER_TOKEN_FILE", "INTEGRATION_BEARER_TOKEN_FILE", "/run/studio/token", func() string { return configFromEnvironment().TokenFile }},
	} {
		t.Run(test.modern, func(t *testing.T) {
			t.Setenv(test.legacy, "legacy")
			t.Setenv(test.modern, test.value)
			if test.read() != test.value {
				t.Fatal("Studio alias did not override legacy value")
			}
			t.Setenv(test.modern, "")
			if test.read() != "" {
				t.Fatal("explicit empty alias must fail closed rather than use legacy identity")
			}
		})
	}
	t.Setenv("STUDIO_BEARER_CAPABILITY", "quarry-discovery")
	if configFromEnvironment().Capability != "quarry-discovery" {
		t.Fatal("capability alias ignored")
	}
}
