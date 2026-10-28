package store_test

import (
	"testing"

	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/store/storetest"
)

func TestMemoryConnectionZDRLifecycleContract(t *testing.T) {
	audit := &auditSeen{}
	storetest.RunConnectionZDRLifecycleContract(t, storetest.ZDRHarness{
		NewRepository:    func(t *testing.T) store.Repository { return store.NewMemoryRepository() },
		AuditEventExists: audit.exists,
	})
}
