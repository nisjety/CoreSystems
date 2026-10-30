package social

import (
	"context"
	"testing"
	"time"
)

type metricsWorkerRepository struct {
	*fakeRepository
	listedOrgIDs []string
}

func (r *metricsWorkerRepository) ListAccounts(ctx context.Context, orgID string) ([]Account, error) {
	r.listedOrgIDs = append(r.listedOrgIDs, orgID)
	return nil, nil
}

type metricsWorkerStore struct {
	fakeMetricsStore
	listAllCalls int
}

func (s *metricsWorkerStore) ListAccountOrgIDs(context.Context) ([]string, error) {
	s.listAllCalls++
	return []string{"org-1", "org-2"}, nil
}

func TestMetricsWorkerSnapshotsOnlyConfiguredTenant(t *testing.T) {
	repo := &metricsWorkerRepository{fakeRepository: newFakeRepository()}
	store := &metricsWorkerStore{}
	service := NewService(repo, WithMetricsStore(store), WithActionExecutor(&fakeCatalogExecutor{}))
	worker := NewMetricsWorker(service, "test", time.Hour, "org-1")
	worker.tick(context.Background())
	if store.listAllCalls != 0 {
		t.Fatalf("all-org enumeration called %d times", store.listAllCalls)
	}
	if len(repo.listedOrgIDs) != 1 || repo.listedOrgIDs[0] != "org-1" {
		t.Fatalf("listed orgs = %v", repo.listedOrgIDs)
	}
}

func TestMetricsWorkerWithoutTenantDoesNotSnapshot(t *testing.T) {
	repo := &metricsWorkerRepository{fakeRepository: newFakeRepository()}
	store := &metricsWorkerStore{}
	service := NewService(repo, WithMetricsStore(store), WithActionExecutor(&fakeCatalogExecutor{}))
	worker := NewMetricsWorker(service, "test", time.Hour, "")
	worker.tick(context.Background())
	if store.listAllCalls != 0 || len(repo.listedOrgIDs) != 0 {
		t.Fatalf("worker without tenant touched org data: all=%d, listed=%v", store.listAllCalls, repo.listedOrgIDs)
	}
}

func TestSnapshotProviderMetricsRejectsEmptyOrgScope(t *testing.T) {
	repo := &metricsWorkerRepository{fakeRepository: newFakeRepository()}
	store := &metricsWorkerStore{}
	service := NewService(repo, WithMetricsStore(store), WithActionExecutor(&fakeCatalogExecutor{}))
	if _, err := service.SnapshotProviderMetrics(context.Background(), ""); err == nil {
		t.Fatal("empty org must fail closed")
	}
	if store.listAllCalls != 0 || len(repo.listedOrgIDs) != 0 {
		t.Fatal("empty org enumerated organization data")
	}
}
