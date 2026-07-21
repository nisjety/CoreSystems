package journey_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/journey"
)

func TestPlan_SendsEnturIdentityAndPreservesTripResponse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("ET-Client-Name"); got != "coresystem-information-core" {
			t.Errorf("ET-Client-Name = %q", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"data":{"trip":{"tripPatterns":[{"duration":120}]}}}`))
	}))
	defer server.Close()

	svc := journey.NewServiceWithURL(http.DefaultClient, cache.New(), server.URL, "coresystem-information-core")
	result, err := svc.Plan(context.Background(), journey.PlanRequest{
		From: journey.Place{ID: "NSR:StopPlace:58404"},
		To:   journey.Place{ID: "NSR:StopPlace:59872"},
	})
	if err != nil {
		t.Fatalf("Plan() error = %v", err)
	}
	if result.Source.Provider != "entur" || result.Source.Dataset != "journey-planner-v3" {
		t.Fatalf("unexpected source: %+v", result.Source)
	}
	if len(result.Data) == 0 {
		t.Fatal("expected response data")
	}
}

func TestPlan_RejectsInvalidPlaces(t *testing.T) {
	svc := journey.NewServiceWithURL(http.DefaultClient, cache.New(), "http://unused", "client")
	_, err := svc.Plan(context.Background(), journey.PlanRequest{From: journey.Place{ID: "bad"}, To: journey.Place{ID: ""}})
	if err == nil {
		t.Fatal("expected missing destination to fail")
	}
}
