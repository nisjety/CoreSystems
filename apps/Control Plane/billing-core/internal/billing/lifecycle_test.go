package billing

import (
	"errors"
	"testing"
)

func TestEnsureBillingOrganizationActiveRejectsTombstone(t *testing.T) {
	if err := ensureBillingOrganizationActive(true); !errors.Is(err, ErrOrganizationDeleted) {
		t.Fatalf("tombstoned organization error = %v; want ErrOrganizationDeleted", err)
	}
}

func TestEnsureBillingOrganizationActiveAllowsLiveOrganization(t *testing.T) {
	if err := ensureBillingOrganizationActive(false); err != nil {
		t.Fatalf("live organization rejected: %v", err)
	}
}
