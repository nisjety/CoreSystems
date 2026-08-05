package nexi

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
)

func TestCreateCheckoutSessionBuildsNexiPayment(t *testing.T) {
	var gotAuth, gotPath, gotMethod string
	var body map[string]any

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotAuth = r.Header.Get("Authorization")
		gotPath = r.URL.Path
		gotMethod = r.Method
		raw, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(raw, &body)
		w.WriteHeader(http.StatusCreated)
		_, _ = w.Write([]byte(`{"paymentId":"pay_123","hostedPaymentPageUrl":"https://pay.example/redirect"}`))
	}))
	defer srv.Close()

	a := NewAdapter(Config{
		BaseURL:              srv.URL,
		SecretKey:            "secret-key-123",
		CheckoutKey:          "checkout-key-pub",
		CheckoutJSURL:        "https://test.checkout.dibspayment.eu/v1/checkout.js?v=1",
		WebhookURL:           "https://verevon.example/api/v1/billing/webhooks/nexi",
		WebhookAuthorization: "wh-secret",
	})

	session, err := a.CreateCheckoutSession(context.Background(), billing.CheckoutParams{
		OrgID:      "org_abc",
		Plan:       "standard",
		SuccessURL: "https://app.verevon/onboarding?checkout=success",
	})
	if err != nil {
		t.Fatalf("CreateCheckoutSession: %v", err)
	}

	// Nexi auth: raw secret, NO Bearer scheme.
	if gotAuth != "secret-key-123" {
		t.Fatalf("Authorization = %q, want raw secret with no scheme", gotAuth)
	}
	if gotMethod != http.MethodPost || gotPath != "/v1/payments" {
		t.Fatalf("request = %s %s, want POST /v1/payments", gotMethod, gotPath)
	}

	order, _ := body["order"].(map[string]any)
	if order["currency"] != "NOK" || order["reference"] != "org_abc" {
		t.Fatalf("order currency/reference wrong: %+v", order)
	}
	if amt, _ := order["amount"].(float64); int64(amt) != 99900 {
		t.Fatalf("order.amount = %v, want 99900 (standard)", order["amount"])
	}
	if body["myReference"] != "org_abc:verevon-standard" {
		t.Fatalf("myReference = %v", body["myReference"])
	}
	// Nexi requires termsUrl for EmbeddedCheckout; with none configured it must
	// be derived from the return URL's origin (never sent empty/omitted).
	checkout, _ := body["checkout"].(map[string]any)
	if checkout["termsUrl"] != "https://app.verevon/terms" {
		t.Fatalf("checkout.termsUrl = %v, want derived https://app.verevon/terms", checkout["termsUrl"])
	}
	if checkout["integrationType"] != "EmbeddedCheckout" {
		t.Fatalf("checkout.integrationType = %v", checkout["integrationType"])
	}
	if _, hasCharge := checkout["charge"]; hasCharge {
		t.Fatalf("checkout must not send a non-standard `charge` field: %+v", checkout)
	}
	notifications, _ := body["notifications"].(map[string]any)
	hooks, _ := notifications["webHooks"].([]any)
	if len(hooks) != 2 {
		t.Fatalf("expected 2 webhooks, got %d", len(hooks))
	}
	first, _ := hooks[0].(map[string]any)
	if first["authorization"] != "wh-secret" {
		t.Fatalf("webhook authorization = %v", first["authorization"])
	}

	if session.Provider != "nexi" || session.PaymentID != "pay_123" {
		t.Fatalf("session provider/paymentId wrong: %+v", session)
	}
	if session.PublishableKey != "checkout-key-pub" || session.ClientURL == "" {
		t.Fatalf("session checkout key/client url missing: %+v", session)
	}
	if session.AmountCents != 99900 || session.Currency != "NOK" {
		t.Fatalf("session amount/currency wrong: %+v", session)
	}
}

func TestRetrieveCheckoutSessionMapsStatus(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/payments/pay_123" {
			t.Errorf("unexpected path %s", r.URL.Path)
		}
		_, _ = w.Write([]byte(`{"payment":{"paymentId":"pay_123","summary":{"reservedAmount":99900,"chargedAmount":99900},"orderDetails":{"amount":99900,"currency":"NOK","reference":"org_abc"},"myReference":"org_abc:verevon-standard"}}`))
	}))
	defer srv.Close()

	a := NewAdapter(Config{BaseURL: srv.URL, SecretKey: "k"})
	status, err := a.RetrieveCheckoutSession(context.Background(), billing.CheckoutLookupParams{PaymentID: "pay_123"})
	if err != nil {
		t.Fatalf("RetrieveCheckoutSession: %v", err)
	}
	if status.Status != "charged" {
		t.Fatalf("status = %q, want charged", status.Status)
	}
	if status.OrgID != "org_abc" || status.Plan != "verevon-standard" {
		t.Fatalf("org/plan = %q/%q", status.OrgID, status.Plan)
	}
	if status.AmountCents != 99900 || status.Provider != "nexi" {
		t.Fatalf("amount/provider wrong: %+v", status)
	}
}

func TestRetrieveCheckoutSessionRejectsUnsafePaymentIDsBeforeProviderCall(t *testing.T) {
	requests := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		requests++
		_, _ = w.Write([]byte(`{"payment":{}}`))
	}))
	defer srv.Close()

	a := NewAdapter(Config{BaseURL: srv.URL, SecretKey: "secret"})
	for _, paymentID := range []string{"../customers", "pay_123?expand=secret", "pay/123", string(make([]byte, 129))} {
		if _, err := a.RetrieveCheckoutSession(context.Background(), billing.CheckoutLookupParams{PaymentID: paymentID}); err == nil {
			t.Fatalf("payment id %q should be rejected", paymentID)
		}
	}
	if requests != 0 {
		t.Fatalf("unsafe ids reached the credentialed provider client: requests=%d", requests)
	}
}

func TestProviderErrorDoesNotExposeResponseBody(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`invalid merchant configuration: provider-secret-detail`))
	}))
	defer srv.Close()

	a := NewAdapter(Config{BaseURL: srv.URL, SecretKey: "secret"})
	_, err := a.RetrieveCheckoutSession(context.Background(), billing.CheckoutLookupParams{PaymentID: "pay_123"})
	if err == nil {
		t.Fatal("expected provider error")
	}
	if strings.Contains(err.Error(), "provider-secret-detail") || strings.Contains(err.Error(), "merchant configuration") {
		t.Fatalf("provider body escaped in error: %v", err)
	}
}

func TestParseMyReference(t *testing.T) {
	cases := map[string][2]string{
		"org_1:verevon-pro": {"org_1", "verevon-pro"},
		"org_2":            {"org_2", ""},
		"":                 {"", ""},
	}
	for in, want := range cases {
		org, plan := parseMyReference(in)
		if org != want[0] || plan != want[1] {
			t.Errorf("parseMyReference(%q) = %q,%q want %q,%q", in, org, plan, want[0], want[1])
		}
	}
}

func TestDeriveTermsURL(t *testing.T) {
	cases := []struct {
		name       string
		configured string
		successURL string
		want       string
	}{
		{"configured wins", "https://verevon.no/vilkar", "https://app.verevon.no/x", "https://verevon.no/vilkar"},
		{"derived from origin", "", "https://app.verevon.no/onboarding?checkout=success", "https://app.verevon.no/terms"},
		{"unparseable falls back to success url", "", "not-a-url", "not-a-url"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := deriveTermsURL(tc.configured, tc.successURL); got != tc.want {
				t.Fatalf("deriveTermsURL(%q,%q) = %q, want %q", tc.configured, tc.successURL, got, tc.want)
			}
		})
	}
}

func TestMissingSecretKeyFailsClosed(t *testing.T) {
	a := NewAdapter(Config{BaseURL: "https://unused"})
	if _, err := a.CreateCheckoutSession(context.Background(), billing.CheckoutParams{OrgID: "o", Plan: "pro", SuccessURL: "https://x"}); err == nil {
		t.Fatal("expected error when secret key missing")
	}
}

func TestNexiAdapterSupportContracts(t *testing.T) {
	a := NewAdapter(Config{})
	customerID, err := a.EnsureCustomer(context.Background(), billing.CustomerInput{OrgID: "  org_1  "})
	if err != nil || customerID != "org_1" {
		t.Fatalf("EnsureCustomer = %q, %v", customerID, err)
	}
	if err := a.ChargeInvoice(context.Background(), billing.Invoice{}); err == nil {
		t.Fatal("direct invoice charging must remain unsupported")
	}
	for plan, amount := range map[string]int64{
		"hobby": 29900, "standard": 99900, "pro": 149900, "enterprise": 249900, "unknown": 0,
	} {
		if got := checkoutPlanAmount(plan); got != amount {
			t.Errorf("checkoutPlanAmount(%q)=%d want %d", plan, got, amount)
		}
	}
	for plan, name := range map[string]string{
		"hobby": "Verevon Essential", "standard": "Verevon Advanced", "pro": "Verevon Expert", "enterprise": "Verevon Custom", "unknown": "Verevon",
	} {
		if got := checkoutPlanDisplayName(plan); got != name {
			t.Errorf("checkoutPlanDisplayName(%q)=%q want %q", plan, got, name)
		}
	}
}
