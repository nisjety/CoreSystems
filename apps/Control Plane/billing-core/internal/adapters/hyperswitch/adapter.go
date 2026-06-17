package hyperswitch

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
)

const (
	defaultBaseURL   = "https://sandbox.hyperswitch.io"
	defaultClientURL = "https://beta.hyperswitch.io/v1/HyperLoader.js"
)

type Config struct {
	BaseURL        string
	APIKey         string
	PublishableKey string
	ProfileID      string
	ClientURL      string
	BackendURL     string
	Timeout        time.Duration
}

type Adapter struct {
	baseURL        string
	apiKey         string
	publishableKey string
	profileID      string
	clientURL      string
	backendURL     string
	httpClient     *http.Client
}

type paymentResponse struct {
	PaymentID    string         `json:"payment_id"`
	ClientSecret string         `json:"client_secret"`
	Status       string         `json:"status"`
	Amount       int64          `json:"amount"`
	Currency     string         `json:"currency"`
	Metadata     map[string]any `json:"metadata"`
}

func NewAdapter(cfg Config) *Adapter {
	if cfg.BaseURL == "" {
		cfg.BaseURL = defaultBaseURL
	}
	if cfg.ClientURL == "" {
		cfg.ClientURL = defaultClientURL
	}
	if cfg.BackendURL == "" {
		cfg.BackendURL = cfg.BaseURL
	}
	if cfg.Timeout <= 0 {
		cfg.Timeout = 10 * time.Second
	}

	return &Adapter{
		baseURL:        strings.TrimRight(cfg.BaseURL, "/"),
		apiKey:         strings.TrimSpace(cfg.APIKey),
		publishableKey: strings.TrimSpace(cfg.PublishableKey),
		profileID:      strings.TrimSpace(cfg.ProfileID),
		clientURL:      strings.TrimSpace(cfg.ClientURL),
		backendURL:     strings.TrimRight(strings.TrimSpace(cfg.BackendURL), "/"),
		httpClient: &http.Client{
			Timeout: cfg.Timeout,
		},
	}
}

func (a *Adapter) ChargeInvoice(ctx context.Context, invoice billing.Invoice) error {
	if a.apiKey == "" {
		return nil
	}
	return fmt.Errorf("hyperswitch invoice charging is not implemented for invoice %s", invoice.InvoiceID)
}

func (a *Adapter) EnsureCustomer(_ context.Context, input billing.CustomerInput) (string, error) {
	if existing := strings.TrimSpace(input.ExistingCustomerID); existing != "" {
		return existing, nil
	}
	if strings.TrimSpace(input.OrgID) == "" {
		return "", fmt.Errorf("org_id is required")
	}

	sum := sha256.Sum256([]byte(input.OrgID))
	return "cus_" + hex.EncodeToString(sum[:])[:32], nil
}

func (a *Adapter) CreateCheckoutSession(
	ctx context.Context,
	params billing.CheckoutParams,
) (billing.CheckoutSession, error) {
	if a.apiKey == "" {
		return billing.CheckoutSession{}, fmt.Errorf("hyperswitch api key missing")
	}
	if a.publishableKey == "" {
		return billing.CheckoutSession{}, fmt.Errorf("hyperswitch publishable key missing")
	}
	if strings.TrimSpace(params.SuccessURL) == "" || strings.TrimSpace(params.CancelURL) == "" {
		return billing.CheckoutSession{}, fmt.Errorf("success and cancel urls are required")
	}

	amount := checkoutPlanAmountNOK(params.Plan)
	if amount <= 0 {
		return billing.CheckoutSession{}, fmt.Errorf("checkout is only supported for paid plans")
	}

	paymentID, err := newPaymentID()
	if err != nil {
		return billing.CheckoutSession{}, err
	}

	metadata := map[string]string{
		"org_id":     params.OrgID,
		"plan":       strings.ToLower(strings.TrimSpace(params.Plan)),
		"source":     "velion",
		"cancel_url": params.CancelURL,
	}
	for key, value := range params.Metadata {
		if strings.TrimSpace(key) == "" || strings.TrimSpace(value) == "" {
			continue
		}
		metadata[key] = value
	}

	payload := map[string]any{
		"payment_id":          paymentID,
		"amount":              amount,
		"amount_to_capture":   amount,
		"currency":            "NOK",
		"capture_method":      "automatic",
		"authentication_type": "three_ds",
		"setup_future_usage":  "off_session",
		"description":         checkoutPlanDisplayName(params.Plan),
		"return_url": appendQuery(params.SuccessURL, map[string]string{
			"checkout":   "success",
			"provider":   "hyperswitch",
			"payment_id": paymentID,
		}),
		"metadata": metadata,
	}
	if customerID := strings.TrimSpace(params.CustomerID); customerID != "" {
		payload["customer_id"] = customerID
	}

	var payment paymentResponse
	if err := a.doJSON(ctx, http.MethodPost, "/payments", payload, &payment); err != nil {
		return billing.CheckoutSession{}, err
	}
	if strings.TrimSpace(payment.PaymentID) == "" {
		payment.PaymentID = paymentID
	}
	if strings.TrimSpace(payment.ClientSecret) == "" {
		return billing.CheckoutSession{}, fmt.Errorf("hyperswitch payment response missing client_secret")
	}

	return billing.CheckoutSession{
		ID:             payment.PaymentID,
		Provider:       "hyperswitch",
		PaymentID:      payment.PaymentID,
		ClientSecret:   payment.ClientSecret,
		PublishableKey: a.publishableKey,
		ClientURL:      a.clientURL,
		BackendURL:     a.backendURL,
		Status:         payment.Status,
		AmountCents:    payment.Amount,
		Currency:       payment.Currency,
	}, nil
}

func (a *Adapter) RetrieveCheckoutSession(
	ctx context.Context,
	params billing.CheckoutLookupParams,
) (billing.CheckoutStatus, error) {
	if a.apiKey == "" {
		return billing.CheckoutStatus{}, fmt.Errorf("hyperswitch api key missing")
	}

	paymentID := strings.TrimSpace(params.PaymentID)
	if paymentID == "" {
		paymentID = paymentIDFromClientSecret(params.ClientSecret)
	}
	if paymentID == "" {
		return billing.CheckoutStatus{}, fmt.Errorf("payment_id is required")
	}

	var payment paymentResponse
	if err := a.doJSON(ctx, http.MethodGet, "/payments/"+url.PathEscape(paymentID), nil, &payment); err != nil {
		return billing.CheckoutStatus{}, err
	}
	if strings.TrimSpace(payment.PaymentID) == "" {
		payment.PaymentID = paymentID
	}

	return billing.CheckoutStatus{
		Provider:     "hyperswitch",
		PaymentID:    payment.PaymentID,
		ClientSecret: payment.ClientSecret,
		Status:       payment.Status,
		OrgID:        metadataString(payment.Metadata, "org_id"),
		Plan:         metadataString(payment.Metadata, "plan"),
		AmountCents:  payment.Amount,
		Currency:     payment.Currency,
	}, nil
}

func (a *Adapter) doJSON(ctx context.Context, method, path string, payload any, out any) error {
	var body io.Reader
	if payload != nil {
		encoded, err := json.Marshal(payload)
		if err != nil {
			return fmt.Errorf("encode hyperswitch request: %w", err)
		}
		body = bytes.NewReader(encoded)
	}

	req, err := http.NewRequestWithContext(ctx, method, a.baseURL+path, body)
	if err != nil {
		return fmt.Errorf("create hyperswitch request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("api-key", a.apiKey)
	if payload != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if a.profileID != "" {
		req.Header.Set("X-Profile-Id", a.profileID)
	}

	resp, err := a.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("hyperswitch request failed: %w", err)
	}
	defer resp.Body.Close()

	responseBody, readErr := io.ReadAll(io.LimitReader(resp.Body, 16*1024))
	if readErr != nil {
		return fmt.Errorf("read hyperswitch response: %w", readErr)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("hyperswitch returned %d: %s", resp.StatusCode, string(responseBody))
	}
	if out == nil {
		return nil
	}
	if err := json.Unmarshal(responseBody, out); err != nil {
		return fmt.Errorf("decode hyperswitch response: %w", err)
	}
	return nil
}

func newPaymentID() (string, error) {
	const alphabet = "abcdefghijklmnopqrstuvwxyz0123456789"
	random := make([]byte, 26)
	if _, err := rand.Read(random); err != nil {
		return "", fmt.Errorf("generate payment id: %w", err)
	}
	for i, value := range random {
		random[i] = alphabet[int(value)%len(alphabet)]
	}
	return "pay_" + string(random), nil
}

func paymentIDFromClientSecret(clientSecret string) string {
	prefix, _, found := strings.Cut(strings.TrimSpace(clientSecret), "_secret_")
	if !found || !strings.HasPrefix(prefix, "pay_") {
		return ""
	}
	return prefix
}

func appendQuery(rawURL string, values map[string]string) string {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return rawURL
	}
	query := parsed.Query()
	for key, value := range values {
		if strings.TrimSpace(key) == "" || strings.TrimSpace(value) == "" {
			continue
		}
		query.Set(key, value)
	}
	parsed.RawQuery = query.Encode()
	return parsed.String()
}

func metadataString(metadata map[string]any, key string) string {
	if metadata == nil {
		return ""
	}
	value, _ := metadata[key].(string)
	return strings.TrimSpace(value)
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
		return "Velion Essential"
	case "standard":
		return "Velion Advanced"
	case "pro":
		return "Velion Expert"
	case "enterprise":
		return "Velion Custom"
	default:
		return "Velion"
	}
}
