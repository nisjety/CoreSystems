package trends

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestProductionNeverCallsExperimentalProvider(t *testing.T) {
	q := Query{Terms: []string{"SEO"}, Geo: "NO", Timeframe: "today 12-m"}
	c := New("production", true)
	c.endpoint = "http://127.0.0.1:1"
	r, e := c.Collect(context.Background(), q)
	if e != nil || r.Status != "disabled" || !ValidReport(r, q, time.Now()) {
		t.Fatal("production fallback enabled", e)
	}
}
func TestClientRejectsMismatchedAndInvalidEvidence(t *testing.T) {
	q := Query{Terms: []string{"SEO"}, Timeframe: "today 12-m"}
	r := Report{Query: q, Source: "pytrends_dev", Scale: "relative_0_100_per_request", Status: "available", ObservedAt: time.Now().UTC().Format(time.RFC3339), Rows: []Row{{Date: time.Now().Add(-24 * time.Hour).Format(time.DateOnly), Values: []int{0}}}}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) { json.NewEncoder(w).Encode(r) }))
	defer server.Close()
	c := New("local", true)
	c.endpoint = server.URL
	if v, e := c.Collect(context.Background(), q); e != nil || v.Rows[0].Values[0] != 0 {
		t.Fatal("observed zero rejected", e)
	}
	r.Rows[0].Values[0] = 101
	if _, e := c.Collect(context.Background(), q); e == nil {
		t.Fatal("invalid index accepted")
	}
	r.Rows[0].Values[0] = 0
	r.Geo = "US"
	if _, e := c.Collect(context.Background(), q); e == nil {
		t.Fatal("different market accepted")
	}
}
