package store_test

import (
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

// D2-016: the in-memory event log meets the per-run sequence contract.
func TestMemoryEventSeq(t *testing.T) {
	t.Parallel()
	storetest.EventSeqConformance(t, func(t *testing.T) store.DB { return store.NewMemory() })
}
