// Booking, label, and tracking against FedEx's Ship API v1
// (POST {base}/ship/v1/shipments) and Track API v1
// (POST {base}/track/v1/trackingnumbers) — reconstructed from FedEx's
// published developer-portal docs and examples, same verification
// standard as wire.go: the envelope shapes below (requestedShipment with
// shipper/recipients/pickupType/serviceType/packagingType/
// requestedPackageLineItems, labelSpecification, and the
// output.transactionShipments/pieceResponses response) are the documented
// common path; MUST be validated against a live sandbox account before
// production use.
//
// LiveBooking gate mirrors DHL/UPS: FedEx has no per-request test flag —
// sandbox (apis-sandbox.fedex.com) vs production (apis.fedex.com) is
// purely which host you call.
package fedex

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
	shipPath  = "/ship/v1/shipments"
	trackPath = "/track/v1/trackingnumbers"
)

var labelCache sync.Map // trackingNumber -> carrier.Label

type shipRequest struct {
	LabelResponseOptions string        `json:"labelResponseOptions"`
	AccountNumber        accountNumber `json:"accountNumber"`
	RequestedShipment    shipShipment  `json:"requestedShipment"`
}

type shipShipment struct {
	Shipper                   partyWithAddress   `json:"shipper"`
	Recipients                []partyWithAddress `json:"recipients"`
	PickupType                string             `json:"pickupType"`
	ServiceType               string             `json:"serviceType"`
	PackagingType             string             `json:"packagingType"`
	ShippingChargesPayment    paymentSection     `json:"shippingChargesPayment"`
	LabelSpecification        labelSpecification `json:"labelSpecification"`
	RequestedPackageLineItems []packageLine      `json:"requestedPackageLineItems"`
}

type paymentSection struct {
	PaymentType string `json:"paymentType"` // "SENDER"
}

type labelSpecification struct {
	ImageType string `json:"imageType"` // "PDF"
}

type shipResponse struct {
	Output shipOutput `json:"output"`
}

type shipOutput struct {
	TransactionShipments []transactionShipment `json:"transactionShipments"`
}

type transactionShipment struct {
	MasterTrackingNumber string          `json:"masterTrackingNumber"`
	PieceResponses       []pieceResponse `json:"pieceResponses"`
}

type pieceResponse struct {
	TrackingNumber   string            `json:"trackingNumber"`
	PackageDocuments []packageDocument `json:"packageDocuments"`
}

type packageDocument struct {
	ContentType  string `json:"contentType"` // "LABEL"
	DocType      string `json:"docType"`     // "PDF"
	EncodedLabel string `json:"encodedLabel"`
}

func (a *Adapter) bookingAllowed() bool {
	return a.config.LiveBooking || strings.Contains(a.config.BaseURL, "sandbox")
}

func (a *Adapter) Book(ctx context.Context, req carrier.BookingRequest) (carrier.Booking, error) {
	if !a.bookingAllowed() {
		return carrier.Booking{}, fmt.Errorf(
			"fedex: booking blocked — BaseURL %q is not the sandbox host (apis-sandbox.fedex.com) "+
				"and LiveBooking is not set; set FEDEX_LIVE_BOOKING=true once validated against a live sandbox account",
			a.config.BaseURL)
	}
	serviceType := req.ServiceName
	if serviceType == "" {
		return carrier.Booking{}, fmt.Errorf("fedex: booking requires the quoted service type")
	}

	payload := shipRequest{
		LabelResponseOptions: "LABEL",
		AccountNumber:        accountNumber{Value: a.config.AccountNumber},
		RequestedShipment: shipShipment{
			Shipper:                partyWithAddress{Address: address{City: req.From.City, PostalCode: req.From.PostalCode, CountryCode: req.From.Country}},
			Recipients:             []partyWithAddress{{Address: address{City: req.To.City, PostalCode: req.To.PostalCode, CountryCode: req.To.Country}}},
			PickupType:             "USE_SCHEDULED_PICKUP",
			ServiceType:            serviceType,
			PackagingType:          "YOUR_PACKAGING",
			ShippingChargesPayment: paymentSection{PaymentType: "SENDER"},
			LabelSpecification:     labelSpecification{ImageType: "PDF"},
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

	body, err := json.Marshal(payload)
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("fedex: encode shipment: %w", err)
	}
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, a.config.BaseURL+shipPath, bytes.NewReader(body))
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("fedex: build shipment request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")

	resp, err := a.client.Do(httpReq)
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("fedex: shipment request failed: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 5<<20))
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("fedex: read shipment response: %w", err)
	}
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		return carrier.Booking{}, fmt.Errorf("fedex: shipment returned %d: %s", resp.StatusCode, raw)
	}

	var parsed shipResponse
	if err := json.Unmarshal(raw, &parsed); err != nil {
		return carrier.Booking{}, fmt.Errorf("fedex: decode shipment response: %w", err)
	}
	if len(parsed.Output.TransactionShipments) == 0 {
		return carrier.Booking{}, fmt.Errorf("fedex: shipment response contained no transactionShipments")
	}
	txn := parsed.Output.TransactionShipments[0]
	if txn.MasterTrackingNumber == "" {
		return carrier.Booking{}, fmt.Errorf("fedex: shipment response missing masterTrackingNumber")
	}

	tracking := txn.MasterTrackingNumber
	for _, piece := range txn.PieceResponses {
		if piece.TrackingNumber != "" {
			tracking = piece.TrackingNumber
		}
		for _, doc := range piece.PackageDocuments {
			if doc.ContentType == "LABEL" && doc.EncodedLabel != "" {
				decoded, decodeErr := base64.StdEncoding.DecodeString(doc.EncodedLabel)
				if decodeErr == nil {
					contentType := "application/pdf"
					if !strings.EqualFold(doc.DocType, "PDF") {
						contentType = "application/octet-stream"
					}
					key := piece.TrackingNumber
					if key == "" {
						key = txn.MasterTrackingNumber
					}
					labelCache.Store(key, carrier.Label{ContentType: contentType, Data: decoded})
				}
			}
		}
	}

	return carrier.Booking{
		CarrierCode: "fedex",
		BookingRef:  txn.MasterTrackingNumber,
		TrackingNo:  tracking,
		Price:       req.Price,
		CreatedAt:   time.Now().UTC(),
	}, nil
}

// Label returns the label FedEx returned inline at booking time. See
// DHL/UPS's Label for the same process-local-cache rationale.
func (a *Adapter) Label(_ context.Context, bookingRef string) (carrier.Label, error) {
	cached, ok := labelCache.Load(bookingRef)
	if !ok {
		return carrier.Label{}, fmt.Errorf("fedex: no label cached for %s (labels are captured inline at booking time)", bookingRef)
	}
	return cached.(carrier.Label), nil
}

type trackRequest struct {
	TrackingInfo []trackingInfoEntry `json:"trackingInfo"`
}

type trackingInfoEntry struct {
	TrackingNumberInfo trackingNumberInfo `json:"trackingNumberInfo"`
}

type trackingNumberInfo struct {
	TrackingNumber string `json:"trackingNumber"`
}

type trackResponse struct {
	Output trackOutput `json:"output"`
}

type trackOutput struct {
	CompleteTrackResults []completeTrackResult `json:"completeTrackResults"`
}

type completeTrackResult struct {
	TrackResults []trackResult `json:"trackResults"`
}

type trackResult struct {
	LatestStatusDetail *statusDetail `json:"latestStatusDetail"`
	ScanEvents         []scanEvent   `json:"scanEvents"`
}

type statusDetail struct {
	Description string `json:"description"`
	Code        string `json:"code"` // "DL" = delivered
}

type scanEvent struct {
	Date             string `json:"date"` // RFC3339
	EventType        string `json:"eventType"`
	EventDescription string `json:"eventDescription"`
}

// Track queries FedEx's Track API v1.
func (a *Adapter) Track(ctx context.Context, trackingNo string) (carrier.TrackingStatus, error) {
	payload, err := json.Marshal(trackRequest{TrackingInfo: []trackingInfoEntry{{TrackingNumberInfo: trackingNumberInfo{TrackingNumber: trackingNo}}}})
	if err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("fedex: encode tracking request: %w", err)
	}
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, a.config.BaseURL+trackPath, bytes.NewReader(payload))
	if err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("fedex: build tracking request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	resp, err := a.client.Do(httpReq)
	if err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("fedex: tracking request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		snippet, _ := io.ReadAll(io.LimitReader(resp.Body, 300))
		return carrier.TrackingStatus{}, fmt.Errorf("fedex: tracking returned %d: %s", resp.StatusCode, snippet)
	}
	var parsed trackResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("fedex: decode tracking response: %w", err)
	}
	if len(parsed.Output.CompleteTrackResults) == 0 || len(parsed.Output.CompleteTrackResults[0].TrackResults) == 0 {
		return carrier.TrackingStatus{}, fmt.Errorf("fedex: tracking returned no results for %s", trackingNo)
	}
	result := parsed.Output.CompleteTrackResults[0].TrackResults[0]

	status := carrier.TrackingStatus{TrackingNo: trackingNo}
	// FedEx returns scan events newest-first; chronological is our contract.
	for i := len(result.ScanEvents) - 1; i >= 0; i-- {
		ev := result.ScanEvents[i]
		occurred, _ := time.Parse(time.RFC3339, ev.Date)
		status.Events = append(status.Events, carrier.TrackingEvent{
			Status:      ev.EventType,
			Description: ev.EventDescription,
			OccurredAt:  occurred,
		})
	}
	if result.LatestStatusDetail != nil {
		status.CurrentStatus = result.LatestStatusDetail.Description
		if strings.EqualFold(result.LatestStatusDetail.Code, "DL") && len(status.Events) > 0 {
			last := status.Events[len(status.Events)-1]
			status.ActualDelivery = &last.OccurredAt
		}
	}
	return status, nil
}
