package hyperswitch

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
)

func TestCreateCheckoutSessionCreatesHyperswitchPayment(t *testing.T) {
	var captured map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/payments" {
			t.Fatalf("unexpected request %s %s", r.Method, r.URL.Path)
		}
		if got := r.Header.Get("api-key"); got != "sk_test_123" {
			t.Fatalf("api-key header = %q", got)
		}
		if got := r.Header.Get("X-Profile-Id"); got != "pro_123" {
			t.Fatalf("X-Profile-Id header = %q", got)
		}
		if err := json.NewDecoder(r.Body).Decode(&captured); err != nil {
			t.Fatalf("decode request: %v", err)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"payment_id":"pay_testpaymentid12345678901234",
			"client_secret":"pay_testpaymentid12345678901234_secret_abc",
			"status":"requires_payment_method",
			"amount":99900,
			"currency":"NOK"
		}`))
	}))
	defer server.Close()

	adapter := NewAdapter(Config{
		BaseURL:        server.URL,
		APIKey:         "sk_test_123",
		PublishableKey: "pk_snd_123",
		ProfileID:      "pro_123",
		ClientURL:      "https://beta.hyperswitch.io/v1/HyperLoader.js",
		BackendURL:     "https://sandbox.hyperswitch.io",
		Timeout:        time.Second,
	})

	session, err := adapter.CreateCheckoutSession(context.Background(), billing.CheckoutParams{
		OrgID:        "org_123",
		Plan:         "standard",
		CustomerID:   "cus_123",
		SuccessURL:   "https://velion.test/onboarding?checkout=success",
		CancelURL:    "https://velion.test/onboarding?checkout=cancel",
		Organization: "Velion Test",
		Metadata:     map[string]string{"source": "onboarding"},
	})
	if err != nil {
		t.Fatalf("CreateCheckoutSession error: %v", err)
	}

	if session.Provider != "hyperswitch" {
		t.Fatalf("Provider = %q", session.Provider)
	}
	if session.ClientSecret != "pay_testpaymentid12345678901234_secret_abc" {
		t.Fatalf("ClientSecret = %q", session.ClientSecret)
	}
	if session.PublishableKey != "pk_snd_123" {
		t.Fatalf("PublishableKey = %q", session.PublishableKey)
	}
	if session.ClientURL != "https://beta.hyperswitch.io/v1/HyperLoader.js" {
		t.Fatalf("ClientURL = %q", session.ClientURL)
	}
	if session.BackendURL != "https://sandbox.hyperswitch.io" {
		t.Fatalf("BackendURL = %q", session.BackendURL)
	}
	if session.AmountCents != 99900 || session.Currency != "NOK" {
		t.Fatalf("amount/currency = %d %s", session.AmountCents, session.Currency)
	}

	if captured["amount"] != float64(99900) {
		t.Fatalf("amount payload = %#v", captured["amount"])
	}
	if captured["currency"] != "NOK" {
		t.Fatalf("currency payload = %#v", captured["currency"])
	}
	if captured["capture_method"] != "automatic" {
		t.Fatalf("capture_method payload = %#v", captured["capture_method"])
	}
	if captured["customer_id"] != "cus_123" {
		t.Fatalf("customer_id payload = %#v", captured["customer_id"])
	}
	if captured["return_url"] == nil || !strings.Contains(captured["return_url"].(string), "payment_id=pay_") {
		t.Fatalf("return_url missing payment_id: %#v", captured["return_url"])
	}

	metadata, ok := captured["metadata"].(map[string]any)
	if !ok {
		t.Fatalf("metadata payload = %#v", captured["metadata"])
	}
	if metadata["org_id"] != "org_123" || metadata["plan"] != "standard" || metadata["source"] != "onboarding" {
		t.Fatalf("metadata = %#v", metadata)
	}
}

func TestRetrieveCheckoutSessionMapsPaymentStatus(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet || r.URL.Path != "/payments/pay_testpaymentid12345678901234" {
			t.Fatalf("unexpected request %s %s", r.Method, r.URL.Path)
		}
		if got := r.Header.Get("api-key"); got != "sk_test_123" {
			t.Fatalf("api-key header = %q", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"payment_id":"pay_testpaymentid12345678901234",
			"client_secret":"pay_testpaymentid12345678901234_secret_abc",
			"status":"succeeded",
			"amount":149900,
			"currency":"NOK",
			"metadata":{"org_id":"org_123","plan":"pro"}
		}`))
	}))
	defer server.Close()

	adapter := NewAdapter(Config{
		BaseURL: server.URL,
		APIKey:  "sk_test_123",
		Timeout: time.Second,
	})

	status, err := adapter.RetrieveCheckoutSession(context.Background(), billing.CheckoutLookupParams{
		PaymentID: "pay_testpaymentid12345678901234",
	})
	if err != nil {
		t.Fatalf("RetrieveCheckoutSession error: %v", err)
	}
	if status.Status != "succeeded" || status.OrgID != "org_123" || status.Plan != "pro" {
		t.Fatalf("status = %#v", status)
	}
	if status.AmountCents != 149900 || status.Currency != "NOK" {
		t.Fatalf("amount/currency = %d %s", status.AmountCents, status.Currency)
	}
}
