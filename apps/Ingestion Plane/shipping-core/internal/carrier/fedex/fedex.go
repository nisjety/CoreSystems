// Package fedex implements carrier.Adapter against FedEx's Rates and
// Transit Times API using OAuth2 client credentials. See wire.go for the
// verification status of the request/response shapes.
package fedex

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"net/http"
	"time"

	"golang.org/x/oauth2"
	"golang.org/x/oauth2/clientcredentials"

	"shipping-core/internal/carrier"
)

const (
	defaultBaseURL = "https://apis.fedex.com"
	ratePath       = "/rate/v1/rates/quotes"
	tokenPath      = "/oauth/token"
)

// Adapter implements carrier.Adapter against the real FedEx API.
type Adapter struct {
	config Config
	client *http.Client
}

// New builds an Adapter. The HTTP client handles the OAuth2
// client-credentials flow (token fetch, caching, refresh) via x/oauth2.
func New(config Config) *Adapter {
	if config.BaseURL == "" {
		config.BaseURL = defaultBaseURL
	}

	cc := clientcredentials.Config{
		ClientID:     config.ClientID,
		ClientSecret: config.ClientSecret,
		TokenURL:     config.BaseURL + tokenPath,
		AuthStyle:    oauth2.AuthStyleInParams, // FedEx wants credentials in the form body
	}
	tokenCtx := context.WithValue(context.Background(), oauth2.HTTPClient, &http.Client{Timeout: 10 * time.Second})

	return &Adapter{
		config: config,
		client: cc.Client(tokenCtx),
	}
}

func (a *Adapter) Info() carrier.Info {
	return carrier.Info{Code: "fedex", Name: "FedEx", Segment: carrier.SegmentB2B}
}

// Quote calls FedEx's Rate API, returning one quote per available FedEx
// service for the route.
func (a *Adapter) Quote(ctx context.Context, req carrier.QuoteRequest) ([]carrier.Quote, error) {
	payload, err := json.Marshal(buildRequest(a.config.AccountNumber, req))
	if err != nil {
		return nil, fmt.Errorf("fedex: encode request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, a.config.BaseURL+ratePath, bytes.NewReader(payload))
	if err != nil {
		return nil, fmt.Errorf("fedex: build request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")

	resp, err := a.client.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("fedex: request failed: %w", err)
	}
	defer resp.Body.Close()

	switch resp.StatusCode {
	case http.StatusOK:
		// fall through
	case http.StatusTooManyRequests:
		return nil, fmt.Errorf("fedex: rate limited (429)")
	case http.StatusUnauthorized, http.StatusForbidden:
		// FedEx's body distinguishes the causes this status can't: a 403
		// FORBIDDEN.ERROR with a token the OAuth endpoint just accepted
		// means the developer-portal project doesn't have the Rates and
		// Transit Times API enabled (portal config, not credentials —
		// live-diagnosed 2026-07-04). Wrong key/base-URL pairs fail at the
		// token endpoint instead and never reach here.
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 2048))
		return nil, fmt.Errorf("fedex: authorization failed (%d): %s — check that the FedEx developer-portal project has the Rates and Transit Times API enabled, and that sandbox keys use https://apis-sandbox.fedex.com", resp.StatusCode, body)
	default:
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 2048))
		return nil, fmt.Errorf("fedex: rates returned %d: %s", resp.StatusCode, body)
	}

	var parsed rateResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return nil, fmt.Errorf("fedex: decode response: %w", err)
	}

	return toDomainQuotes(parsed), nil
}

func buildRequest(account string, req carrier.QuoteRequest) rateRequest {
	return rateRequest{
		AccountNumber: accountNumber{Value: account},
		RequestedShipment: requestedShipment{
			Shipper: partyWithAddress{Address: address{
				City:        req.From.City,
				PostalCode:  req.From.PostalCode,
				CountryCode: req.From.Country,
			}},
			Recipient: partyWithAddress{Address: address{
				City:        req.To.City,
				PostalCode:  req.To.PostalCode,
				CountryCode: req.To.Country,
			}},
			// The user's flow schedules courier pickups (see docs/PRD.md),
			// which this pickupType reflects. Documented as best-effort in
			// wire.go — verify against a live sandbox call.
			PickupType:      "USE_SCHEDULED_PICKUP",
			RateRequestType: []string{"ACCOUNT", "LIST"},
			RequestedPackageLineItems: []packageLine{{
				Weight: weight{Units: "KG", Value: req.Package.WeightKg},
				Dimensions: &dimensions{
					Length: ceilInt(req.Package.LengthCm),
					Width:  ceilInt(req.Package.WidthCm),
					Height: ceilInt(req.Package.HeightCm),
					Units:  "CM",
				},
			}},
		},
	}
}

// ceilInt rounds a dimension up to the next whole cm — FedEx requires
// integer dimensions, and rounding up never understates package size.
func ceilInt(f float64) int {
	return int(math.Ceil(f))
}

func toDomainQuotes(resp rateResponse) []carrier.Quote {
	var quotes []carrier.Quote
	for _, detail := range resp.Output.RateReplyDetails {
		quote := carrier.Quote{
			CarrierCode: "fedex",
			CarrierName: "FedEx",
			ServiceName: pickServiceName(detail),
			Features:    []string{"tracking"},
		}
		if rated, ok := pickRatedDetail(detail.RatedShipmentDetails); ok {
			quote.Price = carrier.Money{
				AmountCents: carrier.FloatToCents(rated.TotalNetCharge),
				Currency:    rated.Currency,
			}
		}
		if days, ok := parseTransitDays(detail.Commit); ok {
			quote.TransitDays = days
			quote.EstimatedDelivery = time.Now().AddDate(0, 0, days)
		}
		quotes = append(quotes, quote)
	}
	return quotes
}

func pickServiceName(detail rateReplyDetail) string {
	if detail.ServiceName != "" {
		return detail.ServiceName
	}
	if detail.ServiceType != "" {
		return "FedEx " + detail.ServiceType
	}
	return "FedEx"
}

// pickRatedDetail prefers the ACCOUNT (negotiated) rate when both ACCOUNT
// and LIST are returned, since that's what would actually be invoiced.
func pickRatedDetail(details []ratedShipmentDetail) (ratedShipmentDetail, bool) {
	if len(details) == 0 {
		return ratedShipmentDetail{}, false
	}
	for _, d := range details {
		if d.RateType == "ACCOUNT" {
			return d, true
		}
	}
	return details[0], true
}

// wordsToDays translates FedEx's enum-style transit strings. Unknown
// values leave transit as 0 (unknown) rather than guessing.
var wordsToDays = map[string]int{
	"ONE_DAY": 1, "TWO_DAYS": 2, "THREE_DAYS": 3, "FOUR_DAYS": 4, "FIVE_DAYS": 5,
	"SIX_DAYS": 6, "SEVEN_DAYS": 7, "EIGHT_DAYS": 8, "NINE_DAYS": 9, "TEN_DAYS": 10,
}

func parseTransitDays(c *commit) (int, bool) {
	if c == nil || c.TransitDays == nil {
		return 0, false
	}
	if days, ok := wordsToDays[c.TransitDays.MinimumTransitTime]; ok {
		return days, true
	}
	return 0, false
}

func (a *Adapter) Book(ctx context.Context, req carrier.BookingRequest) (carrier.Booking, error) {
	return carrier.Booking{}, fmt.Errorf("fedex: booking not implemented yet — see docs/TASKS.md Fase 3")
}

func (a *Adapter) Label(ctx context.Context, bookingRef string) (carrier.Label, error) {
	return carrier.Label{}, fmt.Errorf("fedex: label retrieval not implemented yet — see docs/TASKS.md Fase 3")
}

func (a *Adapter) Track(ctx context.Context, trackingNo string) (carrier.TrackingStatus, error) {
	return carrier.TrackingStatus{}, fmt.Errorf("fedex: tracking not implemented yet — see docs/TASKS.md Fase 3")
}
