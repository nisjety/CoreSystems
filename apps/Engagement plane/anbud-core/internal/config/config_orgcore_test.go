package config

import (
	"strings"
	"testing"
)

const orgCoreToken = "unit-fixture-0123456789abcdef0123456789abcdef"

func TestOrgCoreIsOptional(t *testing.T) {
	t.Parallel()

	cfg, err := LoadFrom(env(required()))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if cfg.OrgCore.Configured() {
		t.Error("org-core must be unconfigured when neither variable is set")
	}
}

func TestOrgCoreLoadsBothValues(t *testing.T) {
	t.Parallel()

	cfg, err := LoadFrom(env(with(required(), map[string]string{
		"ANBUD_ORG_CORE_URL":   "http://org-core:8080",
		"ANBUD_ORG_CORE_TOKEN": orgCoreToken,
	})))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if !cfg.OrgCore.Configured() || cfg.OrgCore.URL != "http://org-core:8080" || cfg.OrgCore.Token.Reveal() != orgCoreToken {
		t.Errorf("OrgCore = %+v", cfg.OrgCore)
	}
}

func TestOrgCoreRefusesHalfAConfigurationAndBadValues(t *testing.T) {
	t.Parallel()

	for name, vars := range map[string]map[string]string{
		"url only":    {"ANBUD_ORG_CORE_URL": "http://org-core:8080"},
		"token only":  {"ANBUD_ORG_CORE_TOKEN": orgCoreToken},
		"short token": {"ANBUD_ORG_CORE_URL": "http://org-core:8080", "ANBUD_ORG_CORE_TOKEN": "too-short-token-value"},
		"url with credentials": {
			"ANBUD_ORG_CORE_URL": "http://user:pw-secret@org-core:8080", "ANBUD_ORG_CORE_TOKEN": orgCoreToken,
		},
	} {
		_, err := LoadFrom(env(with(required(), vars)))
		if err == nil {
			t.Errorf("%s: accepted", name)
			continue
		}
		if (name == "url only" || name == "token only") && !strings.Contains(err.Error(), "must be set together") {
			t.Errorf("%s: error does not say both are needed: %v", name, err)
		}
		if !strings.Contains(err.Error(), "ANBUD_ORG_CORE") {
			t.Errorf("%s: error does not name the variable: %v", name, err)
		}
		for _, secretValue := range []string{orgCoreToken, "too-short-token-value", "pw-secret"} {
			if strings.Contains(err.Error(), secretValue) {
				t.Errorf("%s: error echoes a value", name)
			}
		}
	}
}
