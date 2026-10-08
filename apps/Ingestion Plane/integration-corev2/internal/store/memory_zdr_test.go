package store_test

import (
	"context"
	"sync"
	"testing"

	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/store/storetest"
)

// auditSeen remembers the audit ids a memory repository has handed out when claimed, so the
// suite can ask "was this id committed?" more than once.
type auditSeen struct {
	mu   sync.Mutex
	seen map[store.Repository]map[string]bool
}

func (a *auditSeen) exists(t *testing.T, repo store.Repository, id string) bool {
	t.Helper()
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.seen == nil {
		a.seen = map[store.Repository]map[string]bool{}
	}
	if a.seen[repo] == nil {
		a.seen[repo] = map[string]bool{}
	}
	for {
		event, found, err := repo.ClaimAuditEvent(context.Background())
		if err != nil {
			t.Fatalf("claim audit event: %v", err)
		}
		if !found {
			break
		}
		a.seen[repo][event.ID] = true
	}
	return a.seen[repo][id]
}

func TestMemoryConnectionZDRContract(t *testing.T) {
	audit := &auditSeen{}
	storetest.RunConnectionZDRContract(t, storetest.ZDRHarness{
		NewRepository:    func(t *testing.T) store.Repository { return store.NewMemoryRepository() },
		AuditEventExists: audit.exists,
	})
}
