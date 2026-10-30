package store_test

import (
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

// INNH-9: the in-memory team credit figure counts page_fetched events only.
func TestMemoryCreditUsage(t *testing.T) {
	t.Parallel()
	storetest.CreditUsageConformance(t, func(t *testing.T) store.DB { return store.NewMemory() })
}
