// Package ups implements carrier.Adapter against UPS's Rating API using
// OAuth2 client credentials. See wire.go for the verification status of
// the request/response shapes (verified against UPS's official OpenAPI
// spec, not guessed).
package ups

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"golang.org/x/oauth2"
	"golang.org/x/oauth2/clientcredentials"

	"shipping-core/internal/carrier"
)

const (
	defaultBaseURL = "https://onlinetools.ups.com"
	ratingPath     = "/api/rating/v2409/Shop"
	tokenPath      = "/security/v1/oauth/token"
)

// Adapter implements carrier.Adapter against the real UPS API.
type Adapter struct {
	config Config
	client *http.Client
}

// New builds an Adapter. The returned adapter's HTTP client handles the
// OAuth2 client-credentials flow transparently (token fetch, caching,
// refresh on expiry) via x/oauth2 — the Bearer token never appears in
// adapter code.
func New(config Config) *Adapter {
	if config.BaseURL == "" {
		config.BaseURL = defaultBaseURL
	}

	cc := clientcredentials.Config{
		ClientID:     config.ClientID,
		ClientSecret: config.ClientSecret,
		TokenURL:     config.BaseURL + tokenPath,
		AuthStyle:    oauth2.AuthStyleInHeader, // UPS wants Basic auth on the token request
	}
	// The token fetch inherits this client's timeout; per-request contexts
	// on rating calls still apply to the rating call itself.
	tokenCtx := context.WithValue(context.Background(), oauth2.HTTPClient, &http.Client{Timeout: 10 * time.Second})

	return &Adapter{
		config: config,
		client: cc.Client(tokenCtx),
	}
}

func (a *Adapter) Info() carrier.Info {
	mode := carrier.ModeProduction
	if strings.Contains(strings.ToLower(a.config.BaseURL), "wwwcie") || a.config.BaseURL != defaultBaseURL {
		mode = carrier.ModeSandbox
	}
	return carrier.Info{Code: "ups", Name: "UPS", Segment: carrier.SegmentB2B, Mode: mode}
}

// Quote calls UPS's Rating API with requestoption=Shop, returning one
// quote per available UPS service for the route.
func (a *Adapter) Quote(ctx context.Context, req carrier.QuoteRequest) ([]carrier.Quote, error) {
	payload, err := json.Marshal(buildRequest(a.config.AccountNumber, req))
	if err != nil {
		return nil, fmt.Errorf("ups: encode request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, a.config.BaseURL+ratingPath, bytes.NewReader(payload))
	if err != nil {
		return nil, fmt.Errorf("ups: build request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("transactionSrc", "suplayer-shipping")

	resp, err := a.client.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("ups: request failed: %w", err)
	}
	defer resp.Body.Close()

	switch resp.StatusCode {
	case http.StatusOK:
		// fall through
	case http.StatusTooManyRequests:
		return nil, fmt.Errorf("ups: rate limited (429)")
	case http.StatusUnauthorized, http.StatusForbidden:
		return nil, fmt.Errorf("ups: authentication failed (%d) — check UPS_CLIENT_ID/UPS_CLIENT_SECRET and that the Rating product is enabled on the app", resp.StatusCode)
	default:
		return nil, fmt.Errorf("ups: unexpected status %d", resp.StatusCode)
	}

	var parsed rateResponseEnvelope
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return nil, fmt.Errorf("ups: decode response: %w", err)
	}

	return toDomainQuotes(parsed), nil
}

func buildRequest(accountNumber string, req carrier.QuoteRequest) rateRequestEnvelope {
	from := party{
		Name: req.From.Name,
		Address: address{
			City:        req.From.City,
			PostalCode:  req.From.PostalCode,
			CountryCode: req.From.Country,
		},
	}
	shipper := from
	shipper.ShipperNumber = accountNumber

	return rateRequestEnvelope{RateRequest: rateRequest{
		Request: requestSection{TransactionReference: transactionReference{CustomerContext: "suplayer-shipping"}},
		Shipment: shipment{
			Shipper:  shipper,
			ShipFrom: from,
			ShipTo: party{
				Name: req.To.Name,
				Address: address{
					City:        req.To.City,
					PostalCode:  req.To.PostalCode,
					CountryCode: req.To.Country,
				},
			},
			Package: []packageItem{{
				PackagingType: codeDescription{Code: "02"}, // customer-supplied packaging
				Dimensions: dimensions{
					UnitOfMeasurement: codeDescription{Code: "CM"},
					Length:            formatDim(req.Package.LengthCm),
					Width:             formatDim(req.Package.WidthCm),
					Height:            formatDim(req.Package.HeightCm),
				},
				PackageWeight: packageWeight{
					UnitOfMeasurement: codeDescription{Code: "KGS"},
					Weight:            strconv.FormatFloat(req.Package.WeightKg, 'f', -1, 64),
				},
			}},
		},
	}}
}

// formatDim renders a dimension as UPS expects: a string, at most one
// decimal place.
func formatDim(cm float64) string {
	return strconv.FormatFloat(cm, 'f', 1, 64)
}

func toDomainQuotes(resp rateResponseEnvelope) []carrier.Quote {
	var quotes []carrier.Quote
	for _, rated := range resp.RateResponse.RatedShipment {
		quote := carrier.Quote{
			CarrierCode: "ups",
			CarrierName: "UPS",
			ServiceName: serviceName(rated.Service.Code),
			Features:    []string{"tracking"},
		}
		if rated.TotalCharges != nil {
			quote.Price = carrier.Money{
				AmountCents: carrier.ParseDecimalToCents(rated.TotalCharges.MonetaryValue),
				Currency:    rated.TotalCharges.CurrencyCode,
			}
		}
		if rated.GuaranteedDelivery != nil {
			if days, err := strconv.Atoi(rated.GuaranteedDelivery.BusinessDaysInTransit); err == nil {
				quote.TransitDays = days
				quote.EstimatedDelivery = time.Now().AddDate(0, 0, days)
			}
		}
		quotes = append(quotes, quote)
	}
	return quotes
}

func serviceName(code string) string {
	if name, ok := serviceNames[code]; ok {
		return name
	}
	return "UPS " + code
}

// Book, Label, and Track are implemented in booking.go.
