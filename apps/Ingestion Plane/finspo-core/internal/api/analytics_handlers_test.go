package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/finspo/internal/store"
)

type fakeAnalytics struct {
	largestCall struct {
		org      string
		limit    int
		minBytes int64
	}
	largestOut    []store.LargestItem
	inactiveCall  struct {
		org   string
		older time.Duration
		limit int
	}
	inactiveOut []store.InactiveItem
	bySiteOut   []store.SiteAggregate
	dupsCall    struct {
		org        string
		minCount   int
		minBytes   int64
		maxGroups  int
	}
	dupsOut []store.DuplicateGroup
}

func (f *fakeAnalytics) Largest(_ context.Context, org string, limit int, minBytes int64) ([]store.LargestItem, error) {
	f.largestCall.org = org
	f.largestCall.limit = limit
	f.largestCall.minBytes = minBytes
	return f.largestOut, nil
}

func (f *fakeAnalytics) Inactive(_ context.Context, org string, older time.Duration, limit int) ([]store.InactiveItem, error) {
	f.inactiveCall.org = org
	f.inactiveCall.older = older
	f.inactiveCall.limit = limit
	return f.inactiveOut, nil
}

func (f *fakeAnalytics) BySite(_ context.Context, _ string) ([]store.SiteAggregate, error) {
	return f.bySiteOut, nil
}

func (f *fakeAnalytics) Duplicates(_ context.Context, org string, minCount int, minBytes int64, maxGroups int) ([]store.DuplicateGroup, error) {
	f.dupsCall.org = org
	f.dupsCall.minCount = minCount
	f.dupsCall.minBytes = minBytes
	f.dupsCall.maxGroups = maxGroups
	return f.dupsOut, nil
}

func newAnalyticsServer(a AnalyticsReader) interface {
	Test(*http.Request, ...int) (*http.Response, error)
} {
	return NewServer(ServerConfig{
		APIKey:    "key",
		Browser:   nil,
		Analytics: a,
	})
}

func doAnalyticsRequest(t *testing.T, app interface {
	Test(*http.Request, ...int) (*http.Response, error)
}, path string) *http.Response {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	req.Header.Set("X-API-Key", "key")
	req.Header.Set("X-Org-ID", "org-1")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	return resp
}

func TestLargestUsesDefaults(t *testing.T) {
	t.Parallel()

	fake := &fakeAnalytics{largestOut: []store.LargestItem{{ItemPK: uuid.New(), Name: "x.pdf", SizeBytes: 999}}}
	resp := doAnalyticsRequest(t, newAnalyticsServer(fake), "/api/v1/analytics/largest")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	if fake.largestCall.org != "org-1" || fake.largestCall.limit != 50 || fake.largestCall.minBytes != 0 {
		t.Errorf("call = %#v", fake.largestCall)
	}
}

func TestLargestRespectsExplicitParams(t *testing.T) {
	t.Parallel()

	fake := &fakeAnalytics{}
	resp := doAnalyticsRequest(t, newAnalyticsServer(fake), "/api/v1/analytics/largest?limit=10&min_size=1024")
	defer resp.Body.Close()
	if fake.largestCall.limit != 10 || fake.largestCall.minBytes != 1024 {
		t.Errorf("call = %#v", fake.largestCall)
	}
}

func TestLargestClampsTooLargeLimit(t *testing.T) {
	t.Parallel()

	fake := &fakeAnalytics{}
	resp := doAnalyticsRequest(t, newAnalyticsServer(fake), "/api/v1/analytics/largest?limit=99999")
	defer resp.Body.Close()
	if fake.largestCall.limit != 500 {
		t.Errorf("limit = %d, want clamped to 500", fake.largestCall.limit)
	}
}

func TestInactiveRejectsBadDuration(t *testing.T) {
	t.Parallel()

	resp := doAnalyticsRequest(t, newAnalyticsServer(&fakeAnalytics{}), "/api/v1/analytics/inactive?older_than=not-a-duration")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}

func TestInactiveDefaultAge(t *testing.T) {
	t.Parallel()

	fake := &fakeAnalytics{}
	resp := doAnalyticsRequest(t, newAnalyticsServer(fake), "/api/v1/analytics/inactive")
	defer resp.Body.Close()
	if fake.inactiveCall.older != 180*24*time.Hour {
		t.Errorf("default older = %s, want 180d", fake.inactiveCall.older)
	}
}

func TestDuplicatesEnforcesMinCountTwo(t *testing.T) {
	t.Parallel()

	fake := &fakeAnalytics{}
	resp := doAnalyticsRequest(t, newAnalyticsServer(fake), "/api/v1/analytics/duplicates?min_count=1")
	defer resp.Body.Close()
	if fake.dupsCall.minCount != 2 {
		t.Errorf("minCount = %d, want clamped to 2", fake.dupsCall.minCount)
	}
}

func TestBySiteReturnsPayload(t *testing.T) {
	t.Parallel()

	fake := &fakeAnalytics{bySiteOut: []store.SiteAggregate{{SourceID: uuid.New(), SiteID: "s1", FileCount: 12, TotalBytes: 1024}}}
	resp := doAnalyticsRequest(t, newAnalyticsServer(fake), "/api/v1/analytics/by-site")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	var payload struct {
		Data struct {
			Count int `json:"count"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if payload.Data.Count != 1 {
		t.Errorf("count = %d, want 1", payload.Data.Count)
	}
}
