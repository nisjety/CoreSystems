package social

import (
	"context"
	"testing"
	"time"
)

// fakeMetricsStore is a minimal in-memory MetricsStore for testing the
// service-level ListProviderMetrics wiring (validation + delegation).
// SnapshotProviderMetrics's own collection logic already goes through the
// actions gateway and is covered by live verification, not unit tests here.
type fakeMetricsStore struct {
	rows      []ProviderMetric
	lastQuery ProviderMetricsFilter
}

func (f *fakeMetricsStore) ListAccountOrgIDs(context.Context) ([]string, error) { return nil, nil }
func (f *fakeMetricsStore) UpsertProviderMetrics(context.Context, []ProviderMetric) (int, error) {
	return 0, nil
}
func (f *fakeMetricsStore) ListProviderMetrics(_ context.Context, filter ProviderMetricsFilter) ([]ProviderMetric, error) {
	f.lastQuery = filter
	return f.rows, nil
}

func TestListProviderMetrics_RequiresOrgID(t *testing.T) {
	store := &fakeMetricsStore{}
	svc := NewService(&fakeRepository{}, WithMetricsStore(store))

	if _, err := svc.ListProviderMetrics(context.Background(), ProviderMetricsFilter{}); err == nil {
		t.Fatal("expected an error when org_id is missing")
	}
}

func TestListProviderMetrics_DelegatesToStore(t *testing.T) {
	snapshotDate := time.Date(2026, 7, 4, 0, 0, 0, 0, time.UTC)
	store := &fakeMetricsStore{rows: []ProviderMetric{
		{OrgID: "org-1", AccountID: "acct-1", ProviderKey: "meta", MetricName: "ads.impressions", MetricValue: 1200, SnapshotDate: snapshotDate},
	}}
	svc := NewService(&fakeRepository{}, WithMetricsStore(store))

	rows, err := svc.ListProviderMetrics(context.Background(), ProviderMetricsFilter{
		OrgID:        "  org-1  ",
		AccountID:    "acct-1",
		SnapshotDate: snapshotDate,
	})
	if err != nil {
		t.Fatalf("ListProviderMetrics: %v", err)
	}
	if len(rows) != 1 || rows[0].MetricName != "ads.impressions" {
		t.Fatalf("unexpected rows: %+v", rows)
	}
	if store.lastQuery.OrgID != "org-1" {
		t.Errorf("org_id not trimmed before delegation: %q", store.lastQuery.OrgID)
	}
}

func TestListProviderMetrics_ErrorsWithoutMetricsStore(t *testing.T) {
	svc := NewService(&fakeRepository{})

	if _, err := svc.ListProviderMetrics(context.Background(), ProviderMetricsFilter{OrgID: "org-1"}); err == nil {
		t.Fatal("expected an error when no metrics store is configured")
	}
}
