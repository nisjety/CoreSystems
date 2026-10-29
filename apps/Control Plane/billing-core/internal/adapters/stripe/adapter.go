package stripe

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
)

type Config struct {
	BaseURL string
	APIKey  string
	Timeout time.Duration
}

type Adapter struct {
	baseURL    string
	apiKey     string
	httpClient *http.Client
}

type stripeCustomerResponse struct {
	ID string `json:"id"`
}

type stripeCheckoutSessionResponse struct {
	ID  string `json:"id"`
	URL string `json:"url"`
}

func NewAdapter(cfg Config) *Adapter {
	if cfg.BaseURL == "" {
		cfg.BaseURL = "https://api.stripe.com"
	}
	if cfg.Timeout <= 0 {
		cfg.Timeout = 10 * time.Second
	}

	return &Adapter{
		baseURL: strings.TrimRight(cfg.BaseURL, "/"),
		apiKey:  cfg.APIKey,
		httpClient: &http.Client{
			Timeout: cfg.Timeout,
		},
	}
}

func (a *Adapter) ChargeInvoice(ctx context.Context, invoice billing.Invoice) error {
	if a.apiKey == "" {
		log.Printf("stripe-adapter api key missing, skipping charge for invoice=%s", invoice.InvoiceID)
		return nil
	}

	customerID := extractCustomerID(invoice)
	if customerID == "" {
		return fmt.Errorf("stripe customer id missing for invoice %s", invoice.InvoiceID)
	}

	form := url.Values{}
	form.Set("amount", strconv.FormatInt(invoice.AmountCents, 10))
	form.Set("currency", strings.ToLower(invoice.Currency))
	form.Set("customer", customerID)
	form.Set("confirm", "true")
	form.Set("off_session", "true")
	form.Set("metadata[invoice_id]", invoice.InvoiceID)
	form.Set("metadata[org_id]", invoice.OrgID)

	_, err := a.postForm(ctx, "/v1/payment_intents", form)
	if err != nil {
		return err
	}

	log.Printf("stripe-adapter charged invoice=%s org=%s amount=%d %s", invoice.InvoiceID, invoice.OrgID, invoice.AmountCents, invoice.Currency)
	return nil
}

func (a *Adapter) EnsureCustomer(
	ctx context.Context,
	input billing.CustomerInput,
) (string, error) {
	if a.apiKey == "" {
		if strings.TrimSpace(input.ExistingCustomerID) != "" {
			return strings.TrimSpace(input.ExistingCustomerID), nil
		}
		return "", nil
	}

	form := url.Values{}
	if input.BillingEmail != "" {
		form.Set("email", input.BillingEmail)
	}

	name := strings.TrimSpace(input.OrganizationName)
	if name == "" {
		name = input.OrgID
	}
	form.Set("name", name)
	form.Set("metadata[org_id]", input.OrgID)
	for key, value := range input.Metadata {
		if strings.TrimSpace(key) == "" || strings.TrimSpace(value) == "" {
			continue
		}
		form.Set(fmt.Sprintf("metadata[%s]", key), value)
	}

	path := "/v1/customers"
	if existingCustomerID := strings.TrimSpace(input.ExistingCustomerID); existingCustomerID != "" {
		path += "/" + existingCustomerID
	}

	body, err := a.postForm(ctx, path, form)
	if err != nil {
		return "", err
	}

	var customer stripeCustomerResponse
	if err := json.Unmarshal(body, &customer); err != nil {
		return "", fmt.Errorf("decode stripe customer response: %w", err)
	}
	if strings.TrimSpace(customer.ID) == "" {
		return "", fmt.Errorf("stripe customer id missing in response")
	}
	return customer.ID, nil
}

func (a *Adapter) CreateCheckoutSession(
	ctx context.Context,
	params billing.CheckoutParams,
) (billing.CheckoutSession, error) {
	if a.apiKey == "" {
		return billing.CheckoutSession{}, fmt.Errorf("stripe api key missing")
	}
	if params.CustomerID == "" {
		return billing.CheckoutSession{}, fmt.Errorf("stripe customer id is required")
	}
	if params.SuccessURL == "" || params.CancelURL == "" {
		return billing.CheckoutSession{}, fmt.Errorf("success and cancel urls are required")
	}

	amount := checkoutPlanAmountNOK(params.Plan)
	if amount <= 0 {
		return billing.CheckoutSession{}, fmt.Errorf("checkout is only supported for paid plans")
	}

	form := url.Values{}
	form.Set("mode", "subscription")
	form.Set("customer", params.CustomerID)
	form.Set("success_url", params.SuccessURL)
	form.Set("cancel_url", params.CancelURL)
	form.Set("allow_promotion_codes", "true")
	form.Set("client_reference_id", params.OrgID)
	form.Set("metadata[org_id]", params.OrgID)
	form.Set("metadata[plan]", params.Plan)

	for key, value := range params.Metadata {
		if strings.TrimSpace(key) == "" || strings.TrimSpace(value) == "" {
			continue
		}
		form.Set(fmt.Sprintf("metadata[%s]", key), value)
	}

	form.Set("line_items[0][quantity]", "1")
	form.Set("line_items[0][price_data][currency]", "nok")
	form.Set("line_items[0][price_data][unit_amount]", strconv.FormatInt(amount, 10))
	form.Set("line_items[0][price_data][recurring][interval]", "month")
	form.Set("line_items[0][price_data][product_data][name]", checkoutPlanDisplayName(params.Plan))

	body, err := a.postForm(ctx, "/v1/checkout/sessions", form)
	if err != nil {
		return billing.CheckoutSession{}, err
	}

	var session stripeCheckoutSessionResponse
	if err := json.Unmarshal(body, &session); err != nil {
		return billing.CheckoutSession{}, fmt.Errorf("decode stripe checkout session: %w", err)
	}
	if strings.TrimSpace(session.ID) == "" || strings.TrimSpace(session.URL) == "" {
		return billing.CheckoutSession{}, fmt.Errorf("stripe checkout session response incomplete")
	}

	return billing.CheckoutSession{
		ID:       session.ID,
		URL:      session.URL,
		Provider: "stripe",
	}, nil
}

func (a *Adapter) RetrieveCheckoutSession(
	_ context.Context,
	_ billing.CheckoutLookupParams,
) (billing.CheckoutStatus, error) {
	return billing.CheckoutStatus{}, fmt.Errorf("stripe checkout status retrieval is not implemented")
}

func (a *Adapter) postForm(ctx context.Context, path string, form url.Values) ([]byte, error) {
	endpoint := a.baseURL + path
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, strings.NewReader(form.Encode()))
	if err != nil {
		return nil, fmt.Errorf("create stripe request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+a.apiKey)
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	resp, err := a.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("stripe request failed: %w", err)
	}
	defer resp.Body.Close()

	body, readErr := io.ReadAll(io.LimitReader(resp.Body, 8192))
	if readErr != nil {
		return nil, fmt.Errorf("read stripe response: %w", readErr)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("stripe returned %d: %s", resp.StatusCode, string(body))
	}

	return body, nil
}

func extractCustomerID(invoice billing.Invoice) string {
	if invoice.Metadata == nil {
		return ""
	}
	if value, ok := invoice.Metadata["stripe_customer_id"]; ok {
		if customerID, ok := value.(string); ok {
			return strings.TrimSpace(customerID)
		}
	}
	if value, ok := invoice.Metadata["customer_id"]; ok {
		if customerID, ok := value.(string); ok {
			return strings.TrimSpace(customerID)
		}
	}
	return ""
}

func checkoutPlanAmountNOK(plan string) int64 {
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
