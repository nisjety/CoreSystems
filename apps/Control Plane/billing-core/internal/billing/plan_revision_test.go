package billing

import (
	"context"
	"strings"
	"testing"
)

func TestApplyOrganizationPlanChangeRejectsInvalidBoundaryBeforeStorage(t *testing.T) {
	service := &Service{}
	for _, test := range []struct {
		name     string
		orgID    string
		plan     string
		revision int64
	}{
		{name: "missing org", plan: "pro", revision: 1},
		{name: "zero revision", orgID: "org-a", plan: "pro"},
		{name: "negative revision", orgID: "org-a", plan: "pro", revision: -1},
		{name: "unknown plan", orgID: "org-a", plan: "root", revision: 1},
	} {
		t.Run(test.name, func(t *testing.T) {
			if applied, err := service.ApplyOrganizationPlanChange(
				context.Background(), test.orgID, "", test.plan, test.revision,
			); err == nil || applied {
				t.Fatalf("invalid boundary applied=%t err=%v", applied, err)
			}
		})
	}
}

func TestCloneAccountDoesNotMutateOriginalMaps(t *testing.T) {
	original := Account{
		Products:           map[string]bool{"a": true},
		FeatureFlags:       map[string]bool{"b": true},
		Entitlements:       map[string]bool{"c": true},
		QuotaLimits:        map[string]float64{"d": 1},
		ProviderCustomerID: map[string]string{"e": "provider"},
		Metadata:           map[string]interface{}{"f": "value"},
	}
	cloned := cloneAccount(original)
	cloned.Products["a"] = false
	cloned.FeatureFlags["b"] = false
	cloned.Entitlements["c"] = false
	cloned.QuotaLimits["d"] = 2
	cloned.ProviderCustomerID["e"] = "changed"
	cloned.Metadata["f"] = "changed"
	if !original.Products["a"] || !original.FeatureFlags["b"] || !original.Entitlements["c"] ||
		original.QuotaLimits["d"] != 1 || original.ProviderCustomerID["e"] != "provider" ||
		original.Metadata["f"] != "value" {
		t.Fatalf("clone mutated original account: %+v", original)
	}
}

func TestApplyOrganizationPlanRevisionRejectsInvalidStateBeforeDatabase(t *testing.T) {
	repo := &Repository{}
	if applied, err := repo.ApplyOrganizationPlanRevision(
		context.Background(), Account{OrgID: "org-a"}, 0,
	); err == nil || applied || !strings.Contains(err.Error(), "positive") {
		t.Fatalf("zero repository revision applied=%t err=%v", applied, err)
	}

	if applied, err := repo.ApplyOrganizationPlanRevision(context.Background(), Account{
		OrgID:    "org-a",
		Metadata: map[string]interface{}{"unsupported": make(chan struct{})},
	}, 1); err == nil || applied || !strings.Contains(err.Error(), "marshal metadata") {
		t.Fatalf("unencodable revision state applied=%t err=%v", applied, err)
	}
}

func TestDeactivateOrganizationRejectsMissingOrgBeforeStorage(t *testing.T) {
	service := &Service{}
	if err := service.DeactivateOrganization(context.Background(), " \t ", "deleted"); err == nil {
		t.Fatal("blank organization id was accepted for deactivation")
	}
}
