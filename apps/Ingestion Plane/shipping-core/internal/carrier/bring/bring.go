// Package bring implements carrier.Adapter against Bring's Shipping Guide
// API 2.0 (developer.bring.com/api/shipping-guide_2/). See wire.go for the
// verification status of the request/response shapes used here — the
// response shape is confirmed against Bring's published examples, the
// request shape is a documented best-effort reconstruction pending
// verification against a live Mybring account.
package bring

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"time"

	"shipping-core/internal/carrier"
)

const (
	defaultBaseURL   = "https://api.bring.com/shippingguide/api/v2/products"
	defaultClientURL = "https://velion.no"
)

// knownProductCodes: Bring's Shipping Guide has no "quote every product for
// this lane" mode (see productRequest in wire.go), so a rate-shopping call
// enumerates candidate codes and lets Bring report per-product errors for
// the ones that don't apply to a given lane/customer. List cross-checked
// against verbb/shippy's Bring::getServiceCodes() (a maintained production
// carrier SDK) — https://developer.bring.com/files/Labelspecifications_for_Bring_v_4_991.pdf.
var knownProductCodes = []string{
	"1000", "1002", "1020", "1202", "1206", "1312", "1736", "1885", "1988",
	"3110", "3570", "3584", "4850", "5000", "5100", "5300", "5400", "5600",
	"5800", "9000", "9100", "9300", "9600", "MAIL", "VIP25",
}

// Adapter implements carrier.Adapter against the real Bring API. Never
// registered without valid Config — see NewConfigFromEnv.
type Adapter struct {
	config Config
	client *http.Client
}

// New builds an Adapter. Uses defaultBaseURL unless config.BaseURL is set
// (tests override it to point at an httptest.Server).
func New(config Config) *Adapter {
	if config.BaseURL == "" {
		config.BaseURL = defaultBaseURL
	}
	if config.ClientURL == "" {
		config.ClientURL = defaultClientURL
	}
	return &Adapter{
		config: config,
		client: &http.Client{Timeout: 10 * time.Second},
	}
}

// setAuthHeaders sets the three headers developer.bring.com/api documents as
// ALWAYS required on every Mybring API call: X-Mybring-API-Uid,
// X-Mybring-API-Key, and X-Bring-Client-URL. Shared by Quote (below) and
// Book/Label/Track (booking.go) so no call path can drift and silently omit
// one — the original bug this fixes was exactly that: Quote had two of the
// three, booking.go's own copy had all three.
func (a *Adapter) setAuthHeaders(req *http.Request) {
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	req.Header.Set("X-Mybring-API-Uid", a.config.APIUID)
	req.Header.Set("X-Mybring-API-Key", a.config.APIKey)
	req.Header.Set("X-Bring-Client-URL", a.config.ClientURL)
}

func (a *Adapter) Info() carrier.Info {
	return carrier.Info{Code: "bring", Name: "Bring", Segment: carrier.SegmentBoth}
}

// Quote calls Bring's Shipping Guide API. Respects ctx's deadline —
// quoteengine wraps this in a 4s per-carrier timeout during fan-out, which
// takes effect before the client's own 10s safety-net timeout.
func (a *Adapter) Quote(ctx context.Context, req carrier.QuoteRequest) ([]carrier.Quote, error) {
	payload, err := json.Marshal(buildRequest(a.config.CustomerNumber, req))
	if err != nil {
		return nil, fmt.Errorf("bring: encode request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, a.config.BaseURL, bytes.NewReader(payload))
	if err != nil {
		return nil, fmt.Errorf("bring: build request: %w", err)
	}
	a.setAuthHeaders(httpReq)

	resp, err := a.client.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("bring: request failed: %w", err)
	}
	defer resp.Body.Close()

	switch resp.StatusCode {
	case http.StatusOK:
		// fall through
	case http.StatusTooManyRequests:
		return nil, fmt.Errorf("bring: rate limited (429) — exceeds the documented 120 req/sec")
	default:
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 2048))
		return nil, fmt.Errorf("bring: unexpected status %d: %s", resp.StatusCode, body)
	}

	var parsed shippingGuideResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return nil, fmt.Errorf("bring: decode response: %w", err)
	}

	return toDomainQuotes(parsed), nil
}

func buildRequest(customerNumber string, req carrier.QuoteRequest) shippingGuideRequest {
	products := make([]productRequest, len(knownProductCodes))
	for i, code := range knownProductCodes {
		products[i] = productRequest{ID: code, CustomerNumber: customerNumber}
	}

	return shippingGuideRequest{
		Consignments: []consignmentRequest{{
			ConsignmentID:   "1",
			FromPostalCode:  req.From.PostalCode,
			FromCountryCode: req.From.Country,
			ToPostalCode:    req.To.PostalCode,
			ToCountryCode:   req.To.Country,
			Packages: []packageRequest{{
				ID:          "1",
				GrossWeight: req.Package.WeightKg * 1000, // kg -> grams
				Height:      req.Package.HeightCm,
				Width:       req.Package.WidthCm,
				Length:      req.Package.LengthCm,
			}},
			Products: products,
		}},
	}
}

// toDomainQuotes drops products Bring reports as unavailable/errored for
// this lane (e.g. OUTSIDE_COVERAGE_AREA, INVALID_COUNTRY_PAIR) — expected
// given buildRequest asks about every known product code regardless of
// lane, and only surfaces the ones Bring actually priced.
func toDomainQuotes(resp shippingGuideResponse) []carrier.Quote {
	var quotes []carrier.Quote
	for _, consignment := range resp.Consignments {
		for _, product := range consignment.Products {
			if len(product.Errors) > 0 || product.Price == nil {
				continue
			}
			quotes = append(quotes, toDomainQuote(product))
		}
	}
	return quotes
}

func toDomainQuote(p productResponse) carrier.Quote {
	quote := carrier.Quote{
		CarrierCode: "bring",
		CarrierName: "Bring",
		ServiceName: p.ProductionCode,
		Features:    []string{"tracking"},
	}

	if p.Price != nil {
		quote.Price = carrier.Money{
			AmountCents: carrier.ParseDecimalToCents(p.Price.ListPrice.PriceWithAdditionalServices.AmountWithVAT),
			Currency:    p.Price.ListPrice.CurrencyCode,
		}
	}

	if p.ExpectedDelivery != nil && len(p.ExpectedDelivery.AlternativeDeliveryDates) > 0 {
		first := p.ExpectedDelivery.AlternativeDeliveryDates[0]
		if days, err := strconv.Atoi(first.WorkingDays); err == nil {
			quote.TransitDays = days
		}
		if t, err := time.Parse("02.01.2006", first.FormattedExpectedDeliveryDate); err == nil {
			quote.EstimatedDelivery = t
		}
	}

	return quote
}

// Book, Label, and Track live in booking.go (Bring Booking + Tracking APIs).
