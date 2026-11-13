package config

import "testing"

// WIKI_CREATE_PAGE_PROPOSALS_ENABLED is dark by default and turns on only for the trimmed literal "true".
func TestCreatePageProposalsSwitchIsTheStrictLiteralTrue(t *testing.T) {
	for _, value := range []string{"", "1", "t", "T", "TRUE", "True", "yes", "on", "false", "0", "truee", "\"true\""} {
		t.Setenv("WIKI_CREATE_PAGE_PROPOSALS_ENABLED", value)
		if Load().CreatePageProposalsEnabled {
			t.Errorf("WIKI_CREATE_PAGE_PROPOSALS_ENABLED=%q turned create-page proposals on; only the literal true may", value)
		}
	}
	for _, value := range []string{"true", " true", "true\n"} {
		t.Setenv("WIKI_CREATE_PAGE_PROPOSALS_ENABLED", value)
		if !Load().CreatePageProposalsEnabled {
			t.Errorf("WIKI_CREATE_PAGE_PROPOSALS_ENABLED=%q left create-page proposals off", value)
		}
	}
}
