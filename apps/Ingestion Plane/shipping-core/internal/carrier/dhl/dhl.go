// Package dhl implements carrier.Adapter against MyDHL API's Rating service
// (developer.dhl.com/api-reference/dhl-express-mydhl-api). See wire.go for
// the verification status of the request/response shapes.
package dhl

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"time"

	"shipping-core/internal/carrier"
)

const defaultBaseURL = "https://express.api.dhl.com/mydhlapi"

// Adapter implements carrier.Adapter against the real MyDHL API. Never
// registered without valid Config — see NewConfigFromEnv.
type Adapter struct {
	config Config
	client *http.Client
}

// New builds an Adapter. Uses defaultBaseURL (production) unless
// config.BaseURL is set — a freshly-approved DHL sandbox app should set
// DHL_API_BASE_URL=https://express.api.dhl.com/mydhlapi/test.
func New(config Config) *Adapter {
	if config.BaseURL == "" {
		config.BaseURL = defaultBaseURL
	}
	return &Adapter{
		config: config,
		client: &http.Client{Timeout: 10 * time.Second},
	}
}

func (a *Adapter) Info() carrier.Info {
	return carrier.Info{Code: "dhl", Name: "DHL Express", Segment: carrier.SegmentB2B}
}

// Quote calls MyDHL API's Rating service. MyDHL API uses HTTP Basic Auth —
// the API Key as username, the API Secret as password — with no OAuth2
// flow. Respects ctx's deadline, same as the other real adapters.
func (a *Adapter) Quote(ctx context.Context, req carrier.QuoteRequest) ([]carrier.Quote, error) {
	payload, err := json.Marshal(buildRateRequest(a.config.AccountNumber, req))
	if err != nil {
		return nil, fmt.Errorf("dhl: encode request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, a.config.BaseURL+"/rates", bytes.NewReader(payload))
	if err != nil {
		return nil, fmt.Errorf("dhl: build request: %w", err)
	}
	a.setAuthHeaders(httpReq)

	resp, err := a.client.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("dhl: request failed: %w", err)
	}
	defer resp.Body.Close()

	var parsed rateResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return nil, fmt.Errorf("dhl: decode response: %w", err)
	}

	if resp.StatusCode != http.StatusOK {
		msg := parsed.Detail
		if msg == "" {
			msg = parsed.Status
		}
		return nil, fmt.Errorf("dhl: rates returned %d: %s", resp.StatusCode, msg)
	}

	return toDomainQuotes(parsed), nil
}

func (a *Adapter) setAuthHeaders(req *http.Request) {
	credentials := base64.StdEncoding.EncodeToString([]byte(a.config.APIKey + ":" + a.config.APISecret))
	req.Header.Set("Authorization", "Basic "+credentials)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
}

// nextBusinessDay returns the next weekday after `from`, at a fixed 10:00
// GMT. DHL validates the FULL timestamp against pickup availability, and
// both halves matter — live-observed 2026-07-04 (a Saturday): "tomorrow"
// (a Sunday) fails with error 996 "product(s) not available for the
// requested pickup date", and so does a valid Monday once the clock time
// carried over from now() drifts past the local pickup cutoff (identical
// request succeeded at 16:35 GMT, failed at 16:57 GMT). Fixed mid-morning
// sits inside every European pickup window and makes quotes deterministic
// regardless of when they're requested.
func nextBusinessDay(from time.Time) time.Time {
	next := from.AddDate(0, 0, 1)
	for next.Weekday() == time.Saturday || next.Weekday() == time.Sunday {
		next = next.AddDate(0, 0, 1)
	}
	return time.Date(next.Year(), next.Month(), next.Day(), 10, 0, 0, 0, time.UTC)
}

func buildRateRequest(accountNumber string, req carrier.QuoteRequest) rateRequest {
	rr := rateRequest{
		CustomerDetails: customerDetails{
			ShipperDetails: addressDetails{
				PostalCode:  req.From.PostalCode,
				CityName:    req.From.City,
				CountryCode: req.From.Country,
			},
			ReceiverDetails: addressDetails{
				PostalCode:  req.To.PostalCode,
				CityName:    req.To.City,
				CountryCode: req.To.Country,
			},
		},
		// Omitting productCode returns every product DHL can offer for the
		// route — the correct request shape for a comparison aggregator
		// (vs. pricing one specific pre-chosen service).
		PlannedShippingDateAndTime: nextBusinessDay(time.Now()).Format("2006-01-02T15:04:05 GMT+00:00"),
		UnitOfMeasurement:          "metric",
		IsCustomsDeclarable:        req.From.Country != req.To.Country,
		Packages: []ratePackage{{
			Weight: req.Package.WeightKg,
			Dimensions: &dimensions{
				Length: int(req.Package.LengthCm),
				Width:  int(req.Package.WidthCm),
				Height: int(req.Package.HeightCm),
			},
		}},
	}
	if accountNumber != "" {
		rr.Accounts = []account{{TypeCode: "shipper", Number: accountNumber}}
	}
	return rr
}

func toDomainQuotes(resp rateResponse) []carrier.Quote {
	quotes := make([]carrier.Quote, 0, len(resp.Products))
	for _, p := range resp.Products {
		quotes = append(quotes, toDomainQuote(p))
	}
	return quotes
}

func toDomainQuote(p product) carrier.Quote {
	quote := carrier.Quote{
		CarrierCode: "dhl",
		CarrierName: "DHL Express",
		ServiceName: p.ProductName,
		Features:    []string{"tracking", "express", "international"},
	}
	if len(p.TotalPrice) > 0 {
		quote.Price = carrier.Money{
			AmountCents: carrier.ParseDecimalToCents(p.TotalPrice[0].Price.String()),
			Currency:    p.TotalPrice[0].PriceCurrency,
		}
	}
	if p.DeliveryCapabilities != nil {
		quote.TransitDays = p.DeliveryCapabilities.TotalTransitDays
		if t, err := time.Parse(time.RFC3339, p.DeliveryCapabilities.EstimatedDeliveryDateAndTime); err == nil {
			quote.EstimatedDelivery = t
		}
	}
	return quote
}

// Book, Label, Track, and pickup ordering are NOT implemented yet: unlike
// Bring (where the Booking/Tracking API shapes are cross-verified against
// published examples), MyDHL API's shipment-creation payload has enough
// additional required fields (content type, customs line items, account
// type codes for the shipper vs payer, dangerous-goods declarations) that a
// reconstruction here would be materially less confident than the rating
// shape above — matching the current honest scope of the UPS/FedEx
// adapters. Extend once a live sandbox call has validated the Rating
// integration and the Shipment request shape is confirmed the same way.
func (a *Adapter) Book(_ context.Context, _ carrier.BookingRequest) (carrier.Booking, error) {
	return carrier.Booking{}, fmt.Errorf("dhl: booking not implemented yet")
}

func (a *Adapter) Label(_ context.Context, _ string) (carrier.Label, error) {
	return carrier.Label{}, fmt.Errorf("dhl: label retrieval not implemented yet")
}

func (a *Adapter) Track(_ context.Context, _ string) (carrier.TrackingStatus, error) {
	return carrier.TrackingStatus{}, fmt.Errorf("dhl: tracking not implemented yet")
}
