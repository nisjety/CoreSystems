package store_test

import (
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

// The in-memory store answers the webhook tenancy contract the way Postgres does.
func TestMemory_WebhooksConformance(t *testing.T) {
	storetest.WebhooksConformance(t, func(t *testing.T) store.DB { return store.NewMemory() })
}
