// Booking, label, and tracking against Bring's Booking API and Tracking
// API. VERIFICATION STATUS (same standard as wire.go): the request/response
// shapes below follow developer.bring.com's published Booking API
// (/booking-api/api/booking, schemaVersion 1) and Tracking API
// (/tracking/api/v2/tracking.json) documentation. The response mapping is
// based on Bring's published examples; the exact optional-field spelling of
// the customs block is a documented best-effort reconstruction and MUST be
// validated against a live Mybring test account before production use —
// which is also why testIndicator is hard-true unless Config.LiveBooking is
// explicitly set.
//
// Pickup ordering is deliberately NOT implemented for Bring yet: Bring's
// pickup API surface is the least-documented of the set, and a wrong
// reconstruction would order real-world truck arrivals. The mock fleet
// demonstrates the pickup flow; Bring pickups follow once the shape is
// verified against a real account. (carrier.PickupScheduler is an optional
// interface precisely so this absence is honest, not stubbed.)
package bring

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"
	"time"

	"shipping-core/internal/carrier"
)

const (
	defaultBookingBaseURL  = "https://api.bring.com/booking-api/api/booking"
	defaultTrackingBaseURL = "https://api.bring.com/tracking/api/v2/tracking.json"
)

// labelLinks caches consignmentNumber → label URL from booking
// confirmations. The booking service fetches the label immediately after
// Book (and persists the bytes), so process-local caching suffices; a miss
// after a restart returns an honest error instead of guessing a URL.
var labelLinks sync.Map

type bookingAPIRequest struct {
	TestIndicator bool                    `json:"testIndicator"`
	SchemaVersion int                     `json:"schemaVersion"`
	Consignments  []bookingConsignmentReq `json:"consignments"`
}

type bookingConsignmentReq struct {
	ShippingDateTime string          `json:"shippingDateTime"`
	Parties          bookingParties  `json:"parties"`
	Product          bookingProduct  `json:"product"`
	Packages         []bookingPkgReq `json:"packages"`
}

type bookingParties struct {
	Sender    bookingParty `json:"sender"`
	Recipient bookingParty `json:"recipient"`
}

type bookingParty struct {
	Name                string `json:"name"`
	AddressLine         string `json:"addressLine"`
	PostalCode          string `json:"postalCode"`
	City                string `json:"city"`
	CountryCode         string `json:"countryCode"`
	Reference           string `json:"reference,omitempty"`
	AdditionalAddresses string `json:"additionalAddressInfo,omitempty"`
}

type bookingProduct struct {
	ID             string `json:"id"`
	CustomerNumber string `json:"customerNumber"`
	// CustomsDeclaration: best-effort reconstruction of the international
	// booking block — validate against a live account before use with real
	// cross-border consignments.
	CustomsDeclaration *bookingCustoms `json:"customsDeclaration,omitempty"`
}

type bookingCustoms struct {
	NatureOfTransaction string               `json:"natureOfTransaction,omitempty"`
	Lines               []bookingCustomsLine `json:"customsDeclarationLines"`
}

type bookingCustomsLine struct {
	Description     string  `json:"goodsDescription"`
	Quantity        int     `json:"quantity"`
	CustomsArticleN string  `json:"customsArticleNumber,omitempty"` // HS code
	ItemNetWeightKg float64 `json:"itemNetWeightInKg,omitempty"`
	TariffLineAmt   string  `json:"tariffLineAmount,omitempty"` // decimal string
	Currency        string  `json:"currency,omitempty"`
	CountryOfOrigin string  `json:"countryOfOrigin,omitempty"`
}

type bookingPkgReq struct {
	WeightInKg float64             `json:"weightInKg"`
	Dimensions bookingPkgDimension `json:"dimensions"`
}

type bookingPkgDimension struct {
	HeightInCm float64 `json:"heightInCm"`
	WidthInCm  float64 `json:"widthInCm"`
	LengthInCm float64 `json:"lengthInCm"`
}

type bookingAPIResponse struct {
	Consignments []struct {
		Confirmation *struct {
			ConsignmentNumber string `json:"consignmentNumber"`
			Links             struct {
				Labels string `json:"labels"`
			} `json:"links"`
			Packages []struct {
				PackageNumber string `json:"packageNumber"`
			} `json:"packages"`
		} `json:"confirmation"`
		Errors []struct {
			Code     string `json:"code"`
			Messages []struct {
				Message string `json:"message"`
			} `json:"messages"`
		} `json:"errors"`
	} `json:"consignments"`
}

// Book places the consignment via Bring's Booking API (this call IS the EDI
// pre-advice — Bring's booking API transmits the electronic notification to
// the terminal as part of accepting the order).
func (a *Adapter) Book(ctx context.Context, req carrier.BookingRequest) (carrier.Booking, error) {
	if req.From.Country != req.To.Country && req.Customs == nil {
		return carrier.Booking{}, fmt.Errorf("bring: cross-border booking requires a customs declaration")
	}

	productID := req.ServiceName
	if productID == "" {
		return carrier.Booking{}, fmt.Errorf("bring: booking requires the quoted product id (service name)")
	}

	payload := bookingAPIRequest{
		TestIndicator: !a.config.LiveBooking,
		SchemaVersion: 1,
		Consignments: []bookingConsignmentReq{{
			ShippingDateTime: time.Now().Add(2 * time.Hour).UTC().Format("2006-01-02T15:04:05"),
			Parties: bookingParties{
				Sender:    toBookingParty(req.From),
				Recipient: toBookingParty(req.To),
			},
			Product: bookingProduct{
				ID:                 productID,
				CustomerNumber:     a.config.CustomerNumber,
				CustomsDeclaration: toBookingCustoms(req.Customs),
			},
			Packages: []bookingPkgReq{{
				WeightInKg: req.Package.WeightKg,
				Dimensions: bookingPkgDimension{
					HeightInCm: req.Package.HeightCm,
					WidthInCm:  req.Package.WidthCm,
					LengthInCm: req.Package.LengthCm,
				},
			}},
		}},
	}

	base := a.config.BookingBaseURL
	if base == "" {
		base = defaultBookingBaseURL
	}
	body, err := json.Marshal(payload)
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("bring: encode booking: %w", err)
	}
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, base, bytes.NewReader(body))
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("bring: build booking request: %w", err)
	}
	a.setAuthHeaders(httpReq)

	resp, err := a.client.Do(httpReq)
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("bring: booking request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		snippet, _ := io.ReadAll(io.LimitReader(resp.Body, 300))
		return carrier.Booking{}, fmt.Errorf("bring: booking returned %d: %s", resp.StatusCode, snippet)
	}

	var parsed bookingAPIResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return carrier.Booking{}, fmt.Errorf("bring: decode booking response: %w", err)
	}
	if len(parsed.Consignments) == 0 {
		return carrier.Booking{}, fmt.Errorf("bring: booking response contained no consignments")
	}
	first := parsed.Consignments[0]
	if len(first.Errors) > 0 {
		msg := first.Errors[0].Code
		if len(first.Errors[0].Messages) > 0 {
			msg += ": " + first.Errors[0].Messages[0].Message
		}
		return carrier.Booking{}, fmt.Errorf("bring: booking rejected — %s", msg)
	}
	if first.Confirmation == nil {
		return carrier.Booking{}, fmt.Errorf("bring: booking response missing confirmation")
	}

	tracking := first.Confirmation.ConsignmentNumber
	if len(first.Confirmation.Packages) > 0 && first.Confirmation.Packages[0].PackageNumber != "" {
		tracking = first.Confirmation.Packages[0].PackageNumber
	}
	if first.Confirmation.Links.Labels != "" {
		labelLinks.Store(first.Confirmation.ConsignmentNumber, first.Confirmation.Links.Labels)
	}
	return carrier.Booking{
		CarrierCode: "bring",
		BookingRef:  first.Confirmation.ConsignmentNumber,
		TrackingNo:  tracking,
		Price:       req.Price,
		CreatedAt:   time.Now().UTC(),
	}, nil
}

// Label downloads the label PDF from the link Bring returned at booking
// time. Process-local link cache only — see labelLinks.
func (a *Adapter) Label(ctx context.Context, bookingRef string) (carrier.Label, error) {
	link, ok := labelLinks.Load(bookingRef)
	if !ok {
		return carrier.Label{}, fmt.Errorf("bring: no label link cached for %s (labels are fetched right after booking; re-download via Mybring if this instance restarted)", bookingRef)
	}
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodGet, link.(string), nil)
	if err != nil {
		return carrier.Label{}, fmt.Errorf("bring: build label request: %w", err)
	}
	a.setAuthHeaders(httpReq)
	resp, err := a.client.Do(httpReq)
	if err != nil {
		return carrier.Label{}, fmt.Errorf("bring: label request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return carrier.Label{}, fmt.Errorf("bring: label returned %d", resp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, 5<<20))
	if err != nil {
		return carrier.Label{}, fmt.Errorf("bring: read label: %w", err)
	}
	ct := resp.Header.Get("Content-Type")
	if ct == "" {
		ct = "application/pdf"
	}
	return carrier.Label{ContentType: ct, Data: data}, nil
}

type trackingAPIResponse struct {
	ConsignmentSet []struct {
		PackageSet []struct {
			PackageNumber string `json:"packageNumber"`
			EventSet      []struct {
				Description string `json:"description"`
				Status      string `json:"status"`
				DateISO     string `json:"dateIso"`
			} `json:"eventSet"`
		} `json:"packageSet"`
	} `json:"consignmentSet"`
}

// Track queries Bring's open tracking API for the number's event history.
func (a *Adapter) Track(ctx context.Context, trackingNo string) (carrier.TrackingStatus, error) {
	base := a.config.TrackingBaseURL
	if base == "" {
		base = defaultTrackingBaseURL
	}
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodGet, base+"?q="+trackingNo, nil)
	if err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("bring: build tracking request: %w", err)
	}
	a.setAuthHeaders(httpReq)
	resp, err := a.client.Do(httpReq)
	if err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("bring: tracking request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return carrier.TrackingStatus{}, fmt.Errorf("bring: tracking returned %d", resp.StatusCode)
	}
	var parsed trackingAPIResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("bring: decode tracking response: %w", err)
	}

	status := carrier.TrackingStatus{TrackingNo: trackingNo}
	for _, cons := range parsed.ConsignmentSet {
		for _, pkg := range cons.PackageSet {
			for _, ev := range pkg.EventSet {
				occurred, _ := time.Parse(time.RFC3339, ev.DateISO)
				status.Events = append(status.Events, carrier.TrackingEvent{
					Status:      ev.Status,
					Description: ev.Description,
					OccurredAt:  occurred,
				})
			}
		}
	}
	if len(status.Events) > 0 {
		// Bring returns newest-first; our contract is chronological.
		for i, j := 0, len(status.Events)-1; i < j; i, j = i+1, j-1 {
			status.Events[i], status.Events[j] = status.Events[j], status.Events[i]
		}
		last := status.Events[len(status.Events)-1]
		status.CurrentStatus = last.Status
		if last.Status == "DELIVERED" {
			status.ActualDelivery = &last.OccurredAt
		}
	}
	return status, nil
}

// setAuthHeaders lives in bring.go — shared by Quote, Book, Label, and Track.

func toBookingParty(addr carrier.Address) bookingParty {
	return bookingParty{
		Name:        addr.Name,
		AddressLine: addr.Street,
		PostalCode:  addr.PostalCode,
		City:        addr.City,
		CountryCode: addr.Country,
	}
}

func toBookingCustoms(customs *carrier.CustomsInfo) *bookingCustoms {
	if customs == nil {
		return nil
	}
	lines := make([]bookingCustomsLine, 0, len(customs.Items))
	for _, item := range customs.Items {
		lines = append(lines, bookingCustomsLine{
			Description:     item.Description,
			Quantity:        item.Quantity,
			CustomsArticleN: item.HSCode,
			ItemNetWeightKg: item.WeightKg,
			TariffLineAmt:   fmt.Sprintf("%d.%02d", item.ValueCents/100, item.ValueCents%100),
			Currency:        item.Currency,
			CountryOfOrigin: item.OriginCountry,
		})
	}
	return &bookingCustoms{
		NatureOfTransaction: customs.ContentsType,
		Lines:               lines,
	}
}
