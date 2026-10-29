package exchange_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/exchange"
)

func TestSeries_ValidatesAndForwardsSDMXSeries(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/data/EXR/B.USD.NOK.SP" || r.URL.Query().Get("format") != "sdmx-json" || r.URL.Query().Get("lastNObservations") != "1" {
			t.Errorf("unexpected request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
		if r.URL.EscapedPath() != "/api/data/EXR/B.USD.NOK.SP" {
			t.Errorf("series path must preserve SDMX dimension separator: %s", r.URL.EscapedPath())
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"data":{"dataSets":[]}}`))
	}))
	defer server.Close()

	svc := exchange.NewServiceWithURL(http.DefaultClient, cache.New(), server.URL+"/api/data")
	result, err := svc.Series(context.Background(), exchange.Request{Series: "EXR/B.USD.NOK.SP", LastNObservations: 1})
	if err != nil {
		t.Fatalf("Series() error = %v", err)
	}
	if result.Source.Provider != "norges-bank" || result.QueryHash == "" {
		t.Fatalf("unexpected response metadata: %+v", result)
	}
}

func TestSeries_RejectsUnboundedRequest(t *testing.T) {
	svc := exchange.NewServiceWithURL(http.DefaultClient, cache.New(), "http://unused")
	_, err := svc.Series(context.Background(), exchange.Request{Series: ""})
	if err == nil {
		t.Fatal("expected missing series to fail")
	}
}
