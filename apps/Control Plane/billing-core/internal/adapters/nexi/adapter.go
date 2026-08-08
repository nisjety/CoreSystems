// Package nexi implements the billing PaymentAdapter for Nexi Checkout
// (Nets/Nexi Group), the Nordic payment gateway.
//
// Contract (https://developer.nexigroup.com/nexi-checkout/en-EU/api/):
//   - Base URLs: test https://test.api.dibspayment.eu, live https://api.dibspayment.eu
//   - Auth: the secret API key is sent in the `Authorization` header with NO
//     scheme (no "Bearer"). Server-side only — never exposed to the client.
//   - Create payment: POST /v1/payments (embedded checkout). We register
//     per-payment webhooks via `notifications.webHooks`; each webhook carries an
//     `authorization` string that Nexi echoes back in the webhook's
//     `Authorization` header, which is how we verify inbound webhooks.
//   - Retrieve payment: GET /v1/payments/{paymentId}.
//
// The embedded checkout is completed client-side with the Checkout JS SDK using
// the returned paymentId + the public checkout key; the authoritative plan
// activation happens server-side from the payment.checkout.completed /
// payment.charge.created webhook.
package nexi

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
)

const defaultCheckoutJSURL = "https://checkout.dibspayment.eu/v1/checkout.js?v=1"

type Config struct {
	BaseURL string
	// SecretKey is the Nexi secret API key (Authorization header, no scheme).
	SecretKey string
	// CheckoutKey is the PUBLIC checkout key used by the browser Checkout JS SDK.
	CheckoutKey string
	// CheckoutJSURL is the Checkout JS SDK script URL (test vs live).
	CheckoutJSURL string
	// WebhookURL is the public callback Nexi POSTs payment events to.
	WebhookURL string
	// WebhookAuthorization is the shared secret registered on each webhook and
	// echoed back in the webhook's Authorization header for verification.
	WebhookAuthorization string
	// TermsURL is the merchant terms & conditions URL shown in checkout.
	TermsURL string
	Timeout  time.Duration
}

type Adapter struct {
	cfg        Config
	httpClient *http.Client
}

func NewAdapter(cfg Config) *Adapter {
	if strings.TrimSpace(cfg.BaseURL) == "" {
		cfg.BaseURL = "https://test.api.dibspayment.eu"
	}
	if strings.TrimSpace(cfg.CheckoutJSURL) == "" {
		cfg.CheckoutJSURL = defaultCheckoutJSURL
	}
	if cfg.Timeout <= 0 {
		cfg.Timeout = 10 * time.Second
	}
	cfg.BaseURL = strings.TrimRight(cfg.BaseURL, "/")
	return &Adapter{cfg: cfg, httpClient: &http.Client{Timeout: cfg.Timeout}}
}

// EnsureCustomer — Nexi has no standalone customer object for embedded
// one-off/subscription checkout; the org id is the stable reference we stamp on
// the payment. Return it so the service persists a non-empty customer handle.
func (a *Adapter) EnsureCustomer(_ context.Context, input billing.CustomerInput) (string, error) {
	return strings.TrimSpace(input.OrgID), nil
}

type nexiOrderItem struct {
	Reference        string `json:"reference"`
	Name             string `json:"name"`
	Quantity         int    `json:"quantity"`
	Unit             string `json:"unit"`
	UnitPrice        int64  `json:"unitPrice"`
	GrossTotalAmount int64  `json:"grossTotalAmount"`
	NetTotalAmount   int64  `json:"netTotalAmount"`
}

type nexiWebhook struct {
	EventName     string `json:"eventName"`
	URL           string `json:"url"`
	Authorization string `json:"authorization"`
}

type nexiCreatePaymentRequest struct {
	Order struct {
		Items     []nexiOrderItem `json:"items"`
		Amount    int64           `json:"amount"`
		Currency  string          `json:"currency"`
		Reference string          `json:"reference"`
	} `json:"order"`
	Checkout struct {
		IntegrationType             string `json:"integrationType"`
		URL                         string `json:"url,omitempty"`
		ReturnURL                   string `json:"returnUrl,omitempty"`
		CancelURL                   string `json:"cancelUrl,omitempty"`
		TermsURL                    string `json:"termsUrl"`
		MerchantHandlesConsumerData bool   `json:"merchantHandlesConsumerData"`
	} `json:"checkout"`
	Notifications struct {
		WebHooks []nexiWebhook `json:"webHooks"`
	} `json:"notifications"`
	MyReference string `json:"myReference,omitempty"`
}

type nexiCreatePaymentResponse struct {
	PaymentID            string `json:"paymentId"`
	HostedPaymentPageURL string `json:"hostedPaymentPageUrl"`
}

func (a *Adapter) CreateCheckoutSession(
	ctx context.Context,
	params billing.CheckoutParams,
) (billing.CheckoutSession, error) {
	if strings.TrimSpace(a.cfg.SecretKey) == "" {
		return billing.CheckoutSession{}, fmt.Errorf("nexi secret api key missing")
	}
	if strings.TrimSpace(params.OrgID) == "" {
		return billing.CheckoutSession{}, fmt.Errorf("org id is required")
	}
	if strings.TrimSpace(params.SuccessURL) == "" {
		return billing.CheckoutSession{}, fmt.Errorf("success url is required")
	}

	amount := checkoutPlanAmount(params.Plan)
	if amount <= 0 {
		return billing.CheckoutSession{}, fmt.Errorf("checkout is only supported for paid plans")
	}
	displayName := checkoutPlanDisplayName(params.Plan)

	var req nexiCreatePaymentRequest
	req.Order.Amount = amount
	req.Order.Currency = "NOK"
	req.Order.Reference = params.OrgID
	req.Order.Items = []nexiOrderItem{{
		Reference:        billablePlanReference(params.Plan),
		Name:             displayName,
		Quantity:         1,
		Unit:             "mo",
		UnitPrice:        amount,
		GrossTotalAmount: amount,
		NetTotalAmount:   amount,
	}}
	// EmbeddedCheckout: the browser completes payment inline via the Checkout JS
	// SDK; `url` is the page hosting that SDK (the SPA success/return route).
	// `termsUrl` is REQUIRED by Nexi for embedded checkout — derive it from the
	// return URL's origin when not explicitly configured.
	req.Checkout.IntegrationType = "EmbeddedCheckout"
	req.Checkout.URL = params.SuccessURL
	req.Checkout.TermsURL = deriveTermsURL(a.cfg.TermsURL, params.SuccessURL)
	req.Checkout.MerchantHandlesConsumerData = true
	// Nexi caps myReference at 36 characters. The previous
	// "<orgID>:<planReference>" format was 43-50 chars with 32-char org ids,
	// so Nexi rejected EVERY create with a bare 400 — paid checkout had never
	// once succeeded. The org id is not lost: it already travels in
	// order.reference (set above), which RetrieveCheckoutSession falls back
	// to. myReference therefore carries only the plan ("verevon-<plan>",
	// ≤18 chars), the one value with no other home in Nexi's record.
	req.MyReference = billablePlanReference(params.Plan)

	// Register the two events that authoritatively confirm payment. Only add
	// webhooks when both a public URL and a shared secret are configured, so a
	// misconfigured deploy fails loudly rather than registering an unverifiable
	// (secret-less) webhook.
	if strings.TrimSpace(a.cfg.WebhookURL) != "" && strings.TrimSpace(a.cfg.WebhookAuthorization) != "" {
		for _, ev := range []string{"payment.checkout.completed", "payment.charge.created.v2"} {
			req.Notifications.WebHooks = append(req.Notifications.WebHooks, nexiWebhook{
				EventName:     ev,
				URL:           a.cfg.WebhookURL,
				Authorization: a.cfg.WebhookAuthorization,
			})
		}
	}

	body, err := a.doJSON(ctx, http.MethodPost, "/v1/payments", req)
	if err != nil {
		return billing.CheckoutSession{}, err
	}
	var resp nexiCreatePaymentResponse
	if err := json.Unmarshal(body, &resp); err != nil {
		return billing.CheckoutSession{}, fmt.Errorf("decode nexi create payment: %w", err)
	}
	if strings.TrimSpace(resp.PaymentID) == "" {
		return billing.CheckoutSession{}, fmt.Errorf("nexi create payment response missing paymentId")
	}

	return billing.CheckoutSession{
		ID:             resp.PaymentID,
		PaymentID:      resp.PaymentID,
		Provider:       "nexi",
		PublishableKey: a.cfg.CheckoutKey,
		ClientURL:      a.cfg.CheckoutJSURL,
		URL:            resp.HostedPaymentPageURL,
		Status:         "created",
		AmountCents:    amount,
		Currency:       "NOK",
	}, nil
}

type nexiRetrievePaymentResponse struct {
	Payment struct {
		PaymentID string `json:"paymentId"`
		Summary   struct {
			ReservedAmount int64 `json:"reservedAmount"`
			ChargedAmount  int64 `json:"chargedAmount"`
		} `json:"summary"`
		OrderDetails struct {
			Amount    int64  `json:"amount"`
			Currency  string `json:"currency"`
			Reference string `json:"reference"`
		} `json:"orderDetails"`
		MyReference string `json:"myReference"`
	} `json:"payment"`
}

func (a *Adapter) RetrieveCheckoutSession(
	ctx context.Context,
	params billing.CheckoutLookupParams,
) (billing.CheckoutStatus, error) {
	paymentID := strings.TrimSpace(params.PaymentID)
	if paymentID == "" || len(paymentID) > 128 || !isOpaquePaymentID(paymentID) {
		return billing.CheckoutStatus{}, fmt.Errorf("nexi payment id is invalid or missing")
	}
	if strings.TrimSpace(a.cfg.SecretKey) == "" {
		return billing.CheckoutStatus{}, fmt.Errorf("nexi secret api key missing")
	}

	body, err := a.doJSON(ctx, http.MethodGet, "/v1/payments/"+url.PathEscape(paymentID), nil)
	if err != nil {
		return billing.CheckoutStatus{}, err
	}
	var resp nexiRetrievePaymentResponse
	if err := json.Unmarshal(body, &resp); err != nil {
		return billing.CheckoutStatus{}, fmt.Errorf("decode nexi payment: %w", err)
	}

	p := resp.Payment
	status := "pending"
	switch {
	case p.Summary.ChargedAmount > 0:
		status = "charged"
	case p.Summary.ReservedAmount > 0:
		status = "reserved"
	}
	orgID, plan := parseMyReference(p.MyReference)
	if orgID == "" {
		orgID = strings.TrimSpace(p.OrderDetails.Reference)
	}
	currency := p.OrderDetails.Currency
	if currency == "" {
		currency = "NOK"
	}
	return billing.CheckoutStatus{
		Provider:    "nexi",
		PaymentID:   paymentID,
		Status:      status,
		OrgID:       orgID,
		Plan:        plan,
		AmountCents: p.OrderDetails.Amount,
		Currency:    currency,
	}, nil
}

// ChargeInvoice is not used by the embedded-checkout subscription flow — plan
// activation is driven by the checkout webhook. Recurring charges would use
// Nexi's /v1/subscriptions/charges; not wired for this checkout path.
func (a *Adapter) ChargeInvoice(_ context.Context, _ billing.Invoice) error {
	return fmt.Errorf("nexi: direct invoice charging is not supported by the embedded checkout adapter")
}

// doJSON performs a Nexi API request with the secret key in the Authorization
// header (no scheme, per Nexi's spec).
func (a *Adapter) doJSON(ctx context.Context, method, path string, payload any) ([]byte, error) {
	var reader io.Reader
	if payload != nil {
		buf, err := json.Marshal(payload)
		if err != nil {
			return nil, fmt.Errorf("marshal nexi request: %w", err)
		}
		reader = bytes.NewReader(buf)
	}
	req, err := http.NewRequestWithContext(ctx, method, a.cfg.BaseURL+path, reader)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", a.cfg.SecretKey)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")

	resp, err := a.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("nexi request %s %s: %w", method, path, err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		// Provider bodies may contain merchant configuration or request details.
		// Keep them out of returned errors, which cross the Billing HTTP boundary.
		return nil, fmt.Errorf("nexi %s request returned status %d", method, resp.StatusCode)
	}
	return body, nil
}

func isOpaquePaymentID(value string) bool {
	for _, char := range []byte(value) {
		if !(char >= 'a' && char <= 'z') &&
			!(char >= 'A' && char <= 'Z') &&
			!(char >= '0' && char <= '9') &&
			char != '-' && char != '_' {
			return false
		}
	}
	return true
}

// deriveTermsURL returns the configured terms URL, or synthesizes one from the
// checkout return URL's origin (`<scheme>://<host>/terms`). Nexi requires a
// syntactically valid https terms URL for EmbeddedCheckout; it is not fetched at
// payment-creation time. Falls back to the return URL itself if it cannot be
// parsed into an origin (successURL is validated non-empty by the caller).
func deriveTermsURL(configured, successURL string) string {
	if s := strings.TrimSpace(configured); s != "" {
		return s
	}
	su := strings.TrimSpace(successURL)
	if u, err := url.Parse(su); err == nil && u.Scheme != "" && u.Host != "" {
		return u.Scheme + "://" + u.Host + "/terms"
	}
	return su
}

// parseMyReference recovers org/plan hints from a payment's myReference.
// Three formats exist in the wild:
//   - "verevon-<plan>"       current: plan only; org comes from order.reference
//   - "<orgID>:<planref>"    legacy: both, though only short org ids ever fit
//   - "<orgID>"              oldest: org only
//
// The plan is returned in canonical form ("hobby", not "verevon-hobby") —
// billablePlan() treats any unknown string as "free", so returning the raw
// prefixed reference would silently downgrade a paid activation to a no-op.
func parseMyReference(ref string) (orgID, plan string) {
	ref = strings.TrimSpace(ref)
	if ref == "" {
		return "", ""
	}
	if parts := strings.SplitN(ref, ":", 2); len(parts) == 2 {
		return strings.TrimSpace(parts[0]), canonicalPlanFromReference(parts[1])
	}
	if strings.HasPrefix(ref, "verevon-") {
		return "", canonicalPlanFromReference(ref)
	}
	return ref, ""
}

// canonicalPlanFromReference maps a billable plan reference back to the plan
// id billablePlan()/normalizePlan() understand.
func canonicalPlanFromReference(ref string) string {
	return strings.TrimPrefix(strings.TrimSpace(ref), "verevon-")
}

// checkoutPlanAmount returns the monthly price in minor units (øre), matching
// the Stripe adapter and the frontend plan cards.
func checkoutPlanAmount(plan string) int64 {
	switch strings.ToLower(strings.TrimSpace(plan)) {
	case "hobby":
		return 29900
	case "standard":
		return 99900
	case "pro":
		return 149900
	case "enterprise":
		return 249900
	default:
		return 0
	}
}

func checkoutPlanDisplayName(plan string) string {
	switch strings.ToLower(strings.TrimSpace(plan)) {
	case "hobby":
		return "Verevon Essential"
	case "standard":
		return "Verevon Advanced"
	case "pro":
		return "Verevon Expert"
	case "enterprise":
		return "Verevon Custom"
	default:
		return "Verevon"
	}
}

func billablePlanReference(plan string) string {
	return "verevon-" + strings.ToLower(strings.TrimSpace(plan))
}
