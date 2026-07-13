// Booking, label, and tracking against UPS's Shipping API v1
// (github.com/UPS-API/api-documentation, Shipping.yaml) and Tracking API v1
// (Track.yaml) — same verification standard as wire.go's Rating shapes:
// cross-checked against UPS's published OpenAPI spec, not guessed. Package
// and address types are reused from wire.go (party/address/dimensions/
// packageWeight/codeDescription) since Shipping and Rating share them.
//
// LiveBooking gate mirrors DHL's: UPS's CIE test environment
// (wwwcie.ups.com) and production (onlinetools.ups.com) are two different
// hosts with no per-request test flag, so Book/Label/Track refuse to run
// against a non-CIE BaseURL unless LiveBooking is explicitly true.
//
// International customs (InternationalForms) is not modeled — cross-border
// bookings via UPS should be validated against a live CIE account before
// depending on this for real customs paperwork.
package ups

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"

	"shipping-core/internal/carrier"
)

const (
	shipPath  = "/api/shipments/v2409/ship"
	trackPath = "/api/track/v1/details/"
)

var labelCache sync.Map // trackingNumber -> carrier.Label

type shipmentRequestEnvelope struct {
	ShipmentRequest shipmentRequest `json:"ShipmentRequest"`
}

type shipmentRequest struct {
	Request            requestSection     `json:"Request"`
	Shipment           shipShipment       `json:"Shipment"`
	LabelSpecification labelSpecification `json:"LabelSpecification"`
}

type shipShipment struct {
	Description        string             `json:"Description,omitempty"`
	Shipper            party              `json:"Shipper"`
	ShipTo             party              `json:"ShipTo"`
	ShipFrom           party              `json:"ShipFrom"`
	PaymentInformation paymentInformation `json:"PaymentInformation"`
	Service            codeDescription    `json:"Service"`
	Package            []packageItem      `json:"Package"`
}

type paymentInformation struct {
	ShipmentCharge []shipmentCharge `json:"ShipmentCharge"`
}

type shipmentCharge struct {
	Type        string      `json:"Type"` // "01" = transportation, billed to shipper
	BillShipper billShipper `json:"BillShipper"`
}

type billShipper struct {
	AccountNumber string `json:"AccountNumber"`
}

type labelSpecification struct {
	LabelImageFormat codeDescription `json:"LabelImageFormat"`
}

type shipmentResponseEnvelope struct {
	ShipmentResponse shipmentResponseBody `json:"ShipmentResponse"`
}

type shipmentResponseBody struct {
	ShipmentResults shipmentResults `json:"ShipmentResults"`
}

type shipmentResults struct {
	ShipmentIdentificationNumber string          `json:"ShipmentIdentificationNumber"`
	PackageResults               []packageResult `json:"PackageResults"`
}

type packageResult struct {
	TrackingNumber string        `json:"TrackingNumber"`
	ShippingLabel  shippingLabel `json:"ShippingLabel"`
}

type shippingLabel struct {
	ImageFormat  codeDescription `json:"ImageFormat"`
	GraphicImage string          `json:"GraphicImage"` // base64
}

func (a *Adapter) bookingAllowed() bool {
	return a.config.LiveBooking || strings.Contains(a.config.BaseURL, "wwwcie")
}

func (a *Adapter) Book(ctx context.Context, req carrier.BookingRequest) (carrier.Booking, error) {
	if !a.bookingAllowed() {
		return carrier.Booking{}, fmt.Errorf(
			"ups: booking blocked — BaseURL %q is not the CIE test host (wwwcie.ups.com) "+
				"and LiveBooking is not set; set UPS_LIVE_BOOKING=true once validated against a live CIE account",
			a.config.BaseURL)
	}
	serviceCode := req.ServiceName
	if serviceCode == "" {
		return carrier.Booking{}, fmt.Errorf("ups: booking requires the quoted service code")
	}

	from := party{Name: req.From.Name, Address: address{City: req.From.City, PostalCode: req.From.PostalCode, CountryCode: req.From.Country}}
	shipper := from
	shipper.ShipperNumber = a.config.AccountNumber

	payload := shipmentRequestEnvelope{ShipmentRequest: shipmentRequest{
		Request: requestSection{TransactionReference: transactionReference{CustomerContext: "suplayer-shipping"}},
		Shipment: shipShipment{
			Shipper:  shipper,
			ShipFrom: from,
			ShipTo:   party{Name: req.To.Name, Address: address{City: req.To.City, PostalCode: req.To.PostalCode, CountryCode: req.To.Country}},
			PaymentInformation: paymentInformation{
				ShipmentCharge: []shipmentCharge{{Type: "01", BillShipper: billShipper{AccountNumber: a.config.AccountNumber}}},
			},
			Service: codeDescription{Code: serviceCode},
			Package: []packageItem{{
				PackagingType: codeDescription{Code: "02"},
				Dimensions: dimensions{
					UnitOfMeasurement: codeDescription{Code: "CM"},
					Length:            formatDim(req.Package.LengthCm),
					Width:             formatDim(req.Package.WidthCm),
					Height:            formatDim(req.Package.HeightCm),
				},
				PackageWeight: packageWeight{
					UnitOfMeasurement: codeDescription{Code: "KGS"},
					Weight:            formatDim(req.Package.WeightKg),
				},
			}},
		},
		LabelSpecification: labelSpecification{LabelImageFormat: codeDescription{Code: "GIF"}},
	}}

	body, err := json.Marshal(payload)
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("ups: encode shipment: %w", err)
	}
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, a.config.BaseURL+shipPath, bytes.NewReader(body))
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("ups: build shipment request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("transactionSrc", "suplayer-shipping")

	resp, err := a.client.Do(httpReq)
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("ups: shipment request failed: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 5<<20))
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("ups: read shipment response: %w", err)
	}
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		return carrier.Booking{}, fmt.Errorf("ups: shipment returned %d: %s", resp.StatusCode, raw)
	}

	var parsed shipmentResponseEnvelope
	if err := json.Unmarshal(raw, &parsed); err != nil {
		return carrier.Booking{}, fmt.Errorf("ups: decode shipment response: %w", err)
	}
	results := parsed.ShipmentResponse.ShipmentResults
	if results.ShipmentIdentificationNumber == "" {
		return carrier.Booking{}, fmt.Errorf("ups: shipment response missing ShipmentIdentificationNumber")
	}

	tracking := results.ShipmentIdentificationNumber
	for _, pkg := range results.PackageResults {
		if pkg.TrackingNumber != "" {
			tracking = pkg.TrackingNumber
		}
		if pkg.ShippingLabel.GraphicImage != "" {
			decoded, decodeErr := base64.StdEncoding.DecodeString(pkg.ShippingLabel.GraphicImage)
			if decodeErr == nil {
				contentType := "image/gif"
				if strings.EqualFold(pkg.ShippingLabel.ImageFormat.Code, "PDF") {
					contentType = "application/pdf"
				}
				key := pkg.TrackingNumber
				if key == "" {
					key = results.ShipmentIdentificationNumber
				}
				labelCache.Store(key, carrier.Label{ContentType: contentType, Data: decoded})
			}
		}
	}

	return carrier.Booking{
		CarrierCode: "ups",
		BookingRef:  results.ShipmentIdentificationNumber,
		TrackingNo:  tracking,
		Price:       req.Price,
		CreatedAt:   time.Now().UTC(),
	}, nil
}

// Label returns the label UPS returned inline at booking time (cached by
// tracking number, falling back to the shipment ID). See DHL's Label for
// the same process-local-cache rationale.
func (a *Adapter) Label(_ context.Context, bookingRef string) (carrier.Label, error) {
	cached, ok := labelCache.Load(bookingRef)
	if !ok {
		return carrier.Label{}, fmt.Errorf("ups: no label cached for %s (labels are captured inline at booking time)", bookingRef)
	}
	return cached.(carrier.Label), nil
}

type trackResponseEnvelope struct {
	TrackResponse trackResponseBody `json:"trackResponse"`
}

type trackResponseBody struct {
	Shipment []trackShipment `json:"shipment"`
}

type trackShipment struct {
	Package []trackPackage `json:"package"`
}

type trackPackage struct {
	TrackingNumber string          `json:"trackingNumber"`
	Activity       []trackActivity `json:"activity"`
}

type trackActivity struct {
	Status trackStatus `json:"status"`
	Date   string      `json:"date"` // YYYYMMDD
	Time   string      `json:"time"` // HHMMSS
}

type trackStatus struct {
	Type        string `json:"type"`
	Description string `json:"description"`
	Code        string `json:"code"`
}

// Track queries UPS Tracking API v1.
func (a *Adapter) Track(ctx context.Context, trackingNo string) (carrier.TrackingStatus, error) {
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodGet, a.config.BaseURL+trackPath+trackingNo, nil)
	if err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("ups: build tracking request: %w", err)
	}
	httpReq.Header.Set("transactionSrc", "suplayer-shipping")
	resp, err := a.client.Do(httpReq)
	if err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("ups: tracking request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		snippet, _ := io.ReadAll(io.LimitReader(resp.Body, 300))
		return carrier.TrackingStatus{}, fmt.Errorf("ups: tracking returned %d: %s", resp.StatusCode, snippet)
	}
	var parsed trackResponseEnvelope
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("ups: decode tracking response: %w", err)
	}
	if len(parsed.TrackResponse.Shipment) == 0 || len(parsed.TrackResponse.Shipment[0].Package) == 0 {
		return carrier.TrackingStatus{}, fmt.Errorf("ups: tracking returned no packages for %s", trackingNo)
	}
	pkg := parsed.TrackResponse.Shipment[0].Package[0]

	status := carrier.TrackingStatus{TrackingNo: trackingNo}
	// UPS returns activity newest-first; chronological is our contract.
	for i := len(pkg.Activity) - 1; i >= 0; i-- {
		act := pkg.Activity[i]
		occurred := parseUPSDateTime(act.Date, act.Time)
		status.Events = append(status.Events, carrier.TrackingEvent{
			Status:      act.Status.Type,
			Description: act.Status.Description,
			OccurredAt:  occurred,
		})
	}
	if len(pkg.Activity) > 0 {
		latest := pkg.Activity[0]
		status.CurrentStatus = latest.Status.Description
		if strings.EqualFold(latest.Status.Type, "D") || strings.Contains(strings.ToLower(latest.Status.Description), "delivered") {
			delivered := parseUPSDateTime(latest.Date, latest.Time)
			status.ActualDelivery = &delivered
		}
	}
	return status, nil
}

func parseUPSDateTime(date, timeStr string) time.Time {
	if len(date) != 8 {
		return time.Time{}
	}
	if len(timeStr) != 6 {
		timeStr = "000000"
	}
	t, _ := time.Parse("20060102150405", date+timeStr)
	return t
}
