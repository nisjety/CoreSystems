package officialseries

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestFetchSSBPostsBoundedJSONStatQuery(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Fatalf("method = %s, want POST", r.Method)
		}
		if r.URL.Path != "/tables/07459/data" {
			t.Fatalf("path = %q", r.URL.Path)
		}
		if got := r.URL.Query().Get("outputFormat"); got != "json-stat2" {
			t.Fatalf("outputFormat = %q", got)
		}
		if got := r.URL.Query().Get("lang"); got != "en" {
			t.Fatalf("lang = %q", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"class":"dataset","id":["ContentsCode"],"value":[1]}`))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL, server.URL)
	snapshot, err := client.FetchSSB(context.Background(), SSBQuery{
		TableID:   "07459",
		Selection: []SSBSelection{{VariableCode: "ContentsCode", ValueCodes: []string{"KPI"}}},
	})
	if err != nil {
		t.Fatalf("FetchSSB() error = %v", err)
	}
	if snapshot.Provider != "ssb" || snapshot.Dataset != "pxwebapi-v2" {
		t.Fatalf("source = %s/%s", snapshot.Provider, snapshot.Dataset)
	}
	if snapshot.QueryHash == "" || snapshot.RetrievedAt.IsZero() {
		t.Fatalf("snapshot metadata incomplete: %+v", snapshot)
	}
	if string(snapshot.Payload) == "" {
		t.Fatal("snapshot payload is empty")
	}
}

func TestFetchNorgesBankUsesBoundedSeriesWindow(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			t.Fatalf("method = %s, want GET", r.Method)
		}
		if r.URL.Path != "/api/data/EXR/B.NOK.EUR.SP" {
			t.Fatalf("path = %q", r.URL.Path)
		}
		query := r.URL.Query()
		if query.Get("format") != "sdmx-json" || query.Get("startPeriod") != "2026-01-01" || query.Get("endPeriod") != "2026-01-31" {
			t.Fatalf("query = %v", query)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"data":{"dataSets":[{}]}}`))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL, server.URL)
	snapshot, err := client.FetchNorgesBank(context.Background(), NorgesBankRequest{
		Series:      "EXR/B.NOK.EUR.SP",
		StartPeriod: "2026-01-01",
		EndPeriod:   "2026-01-31",
	})
	if err != nil {
		t.Fatalf("FetchNorgesBank() error = %v", err)
	}
	if snapshot.Provider != "norges-bank" || snapshot.License != "NLOD-2.0" {
		t.Fatalf("source = %s/%s", snapshot.Provider, snapshot.License)
	}
}

func TestCuratedRequestsRejectUnboundedOrInvalidInputs(t *testing.T) {
	client := NewClient(http.DefaultClient, "http://127.0.0.1:1", "http://127.0.0.1:1")
	if _, err := client.FetchSSB(context.Background(), SSBQuery{TableID: "1234"}); err == nil {
		t.Fatal("FetchSSB() error = nil for invalid table")
	}
	if _, err := client.FetchNorgesBank(context.Background(), NorgesBankRequest{Series: "EXR/B.NOK.EUR.SP"}); err == nil {
		t.Fatal("FetchNorgesBank() error = nil for unbounded request")
	}
	if _, err := client.FetchNorgesBank(context.Background(), NorgesBankRequest{Series: strings.Repeat("x", 161), LastNObservations: 10}); err == nil {
		t.Fatal("FetchNorgesBank() error = nil for oversized series")
	}
}

func TestCollectorsRejectOversizedPayloads(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(strings.Repeat("x", 64)))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL, server.URL)
	client.MaxResponseBytes = 32
	_, err := client.FetchNorgesBank(context.Background(), NorgesBankRequest{Series: "EXR/B.NOK.EUR.SP", LastNObservations: 1})
	if err == nil {
		t.Fatal("FetchNorgesBank() error = nil for oversized payload")
	}
}
