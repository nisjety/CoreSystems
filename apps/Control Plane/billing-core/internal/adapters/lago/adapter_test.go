package lago

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
)

func TestReportUsageFailsClosedWithoutCredential(t *testing.T) {
	adapter := NewAdapter(Config{APIKey: ""})
	err := adapter.ReportUsage(context.Background(), billing.UsageEvent{
		EventID:    "usage_01",
		OrgID:      "org-usage",
		Metric:     "api_calls",
		Quantity:   1,
		OccurredAt: time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC),
	})
	if err == nil {
		t.Fatal("missing Lago credential marked durable usage delivered")
	}
}

func TestReportUsageUsesStableEventIDAsTransactionID(t *testing.T) {
	transactionIDs := make([]string, 0, 2)
	server := httptest.NewServer(http.HandlerFunc(func(response http.ResponseWriter, request *http.Request) {
		if request.Header.Get("Authorization") != "Bearer fixture-key" {
			t.Errorf("authorization=%q", request.Header.Get("Authorization"))
		}
		var payload struct {
			Event struct {
				TransactionID string `json:"transaction_id"`
			} `json:"event"`
		}
		if err := json.NewDecoder(request.Body).Decode(&payload); err != nil {
			t.Errorf("decode Lago payload: %v", err)
			response.WriteHeader(http.StatusBadRequest)
			return
		}
		transactionIDs = append(transactionIDs, payload.Event.TransactionID)
		response.WriteHeader(http.StatusOK)
	}))
	defer server.Close()

	adapter := NewAdapter(Config{BaseURL: server.URL, APIKey: "fixture-key"})
	usage := billing.UsageEvent{
		EventID:    "usage_retry_01",
		OrgID:      "org-usage",
		Metric:     "api_calls",
		Quantity:   1,
		Source:     "model-plane",
		OccurredAt: time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC),
	}
	for attempt := 0; attempt < 2; attempt++ {
		if err := adapter.ReportUsage(context.Background(), usage); err != nil {
			t.Fatalf("report attempt %d: %v", attempt+1, err)
		}
	}
	if len(transactionIDs) != 2 || transactionIDs[0] != usage.EventID || transactionIDs[1] != usage.EventID {
		t.Fatalf("Lago transaction IDs=%v; want stable %q", transactionIDs, usage.EventID)
	}
}
