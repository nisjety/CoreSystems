package statistics_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/statistics"
)

func TestQuery_SendsBoundedPxWebV2SelectionAndPreservesJSONStat(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Errorf("method = %s, want POST", r.Method)
		}
		if r.URL.Path != "/tables/05810/data" || r.URL.Query().Get("outputFormat") != "json-stat2" {
			t.Errorf("unexpected request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"class":"dataset","id":["Tid"],"size":[1],"value":[123]}`))
	}))
	defer server.Close()

	svc := statistics.NewServiceWithURL(http.DefaultClient, cache.New(), server.URL)
	result, err := svc.Query(context.Background(), statistics.QueryRequest{
		TableID:   "05810",
		Selection: []statistics.Selection{{VariableCode: "Tid", ValueCodes: []string{"top(1)"}}},
	})
	if err != nil {
		t.Fatalf("Query() error = %v", err)
	}
	if string(result.Data) != `{"class":"dataset","id":["Tid"],"size":[1],"value":[123]}` {
		t.Fatalf("unexpected data: %s", result.Data)
	}
	if result.TableID != "05810" || result.QueryHash == "" {
		t.Fatalf("missing reproducibility metadata: %+v", result)
	}
	if result.Source.Provider != "ssb" || result.Source.Dataset != "pxwebapi-v2" {
		t.Fatalf("unexpected source: %+v", result.Source)
	}
}

func TestQuery_RejectsUnboundedSelection(t *testing.T) {
	svc := statistics.NewServiceWithURL(http.DefaultClient, cache.New(), "http://unused")
	_, err := svc.Query(context.Background(), statistics.QueryRequest{TableID: "05810"})
	if err == nil {
		t.Fatal("expected missing selection to fail")
	}
	_, err = svc.Query(context.Background(), statistics.QueryRequest{
		TableID:   "not-a-table",
		Selection: []statistics.Selection{{VariableCode: "Tid", ValueCodes: []string{"top(1)"}}},
	})
	if err == nil {
		t.Fatal("expected invalid table id to fail")
	}
}

func TestMetadata_ReturnsRawOfficialMetadata(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet || r.URL.Path != "/tables/05810/metadata" {
			t.Errorf("unexpected request: %s %s", r.Method, r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"id":["Tid"],"size":[50]}`))
	}))
	defer server.Close()

	svc := statistics.NewServiceWithURL(http.DefaultClient, cache.New(), server.URL)
	result, err := svc.Metadata(context.Background(), "05810")
	if err != nil {
		t.Fatalf("Metadata() error = %v", err)
	}
	if string(result.Data) != `{"id":["Tid"],"size":[50]}` {
		t.Fatalf("unexpected metadata: %s", result.Data)
	}
}
