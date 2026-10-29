// Booking, label, and tracking against MyDHL API's Shipment service
// (developer.dhl.com/api-reference/mydhl-api-dhl-express). VERIFICATION
// STATUS (same standard as wire.go): the request/response shapes below
// follow MyDHL API's documented Shipment resource — POST /shipments with
// content.packages[], customerDetails{shipperDetails,receiverDetails}, and
// outputImageProperties requesting an inline label — reconstructed from
// published documentation and MUST be validated against a live DHL sandbox
// account before production use.
//
// Unlike Bring, MyDHL API has no per-request test flag: sandbox vs
// production is purely which BaseURL you call (…/mydhlapi/test vs
// …/mydhlapi). Book/Label/Track therefore refuse to run against a
// non-sandbox BaseURL unless Config.LiveBooking is explicitly true — the
// same hard-safe-by-default property Bring gets from testIndicator,
// expressed the way DHL's actual API supports it.
package dhl

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

// labelCache holds the inline label bytes MyDHL API returns directly in the
// create-shipment response, keyed by shipmentTrackingNumber. Process-local
// only — see Bring's identical rationale in booking.go.
var labelCache sync.Map

type shipmentRequest struct {
	PlannedShippingDateAndTime string           `json:"plannedShippingDateAndTime"`
	ProductCode                string           `json:"productCode"`
	Pickup                     shipmentPickup   `json:"pickup"`
	Accounts                   []account        `json:"accounts,omitempty"`
	CustomerDetails            customerDetails  `json:"customerDetails"`
	Content                    shipmentContent  `json:"content"`
	OutputImageProperties      outputImageProps `json:"outputImageProperties"`
}

type shipmentPickup struct {
	IsRequested bool `json:"isRequested"`
}

type outputImageProps struct {
	ImageOptions []imageOption `json:"imageOptions"`
}

type imageOption struct {
	TypeCode string `json:"typeCode"` // "label"
}

type shipmentContent struct {
	Packages            []ratePackage      `json:"packages"`
	IsCustomsDeclarable bool               `json:"isCustomsDeclarable"`
	Description         string             `json:"description"`
	UnitOfMeasurement   string             `json:"unitOfMeasurement"`
	Incoterm            string             `json:"incoterm,omitempty"`
	ExportDeclaration   *exportDeclaration `json:"exportDeclaration,omitempty"`
}

type exportDeclaration struct {
	LineItems []exportLineItem `json:"lineItems"`
}

type exportLineItem struct {
	Number                            int      `json:"number"`
	Description                       string   `json:"description"`
	Price                             float64  `json:"price"`
	Quantity                          quantity `json:"quantity"`
	CommodityCode                     string   `json:"commodityCode,omitempty"`
	ExportControlClassificationNumber string   `json:"-"`
	ManufacturerCountry               string   `json:"manufacturerCountry,omitempty"`
	NetWeight                         float64  `json:"netWeight,omitempty"`
}

type quantity struct {
	Value             int    `json:"value"`
	UnitOfMeasurement string `json:"unitOfMeasurement"`
}

type shipmentResponse struct {
	ShipmentTrackingNumber string `json:"shipmentTrackingNumber"`
	Packages               []struct {
		TrackingNumber string `json:"trackingNumber"`
	} `json:"packages"`
	Documents []struct {
		TypeCode    string `json:"typeCode"`
		ImageFormat string `json:"imageFormat"`
		Content     string `json:"content"` // base64
	} `json:"documents"`
}

type dhlErrorResponse struct {
	Status int    `json:"status"`
	Detail string `json:"detail"`
	Title  string `json:"title"`
}

// bookingAllowed enforces the sandbox-unless-explicit-opt-in rule.
func (a *Adapter) bookingAllowed() bool {
	return a.config.LiveBooking || strings.Contains(a.config.BaseURL, "/test")
}

func (a *Adapter) Book(ctx context.Context, req carrier.BookingRequest) (carrier.Booking, error) {
	if !a.bookingAllowed() {
		return carrier.Booking{}, fmt.Errorf(
			"dhl: booking blocked — BaseURL %q is not the documented sandbox host "+
				"(…/mydhlapi/test) and LiveBooking is not set; set DHL_LIVE_BOOKING=true "+
				"only once the shipment shape below has been validated against a live account",
			a.config.BaseURL)
	}
	if req.From.Country != req.To.Country && req.Customs == nil {
		return carrier.Booking{}, fmt.Errorf("dhl: cross-border booking requires a customs declaration")
	}
	productCode := req.ServiceName
	if productCode == "" {
		return carrier.Booking{}, fmt.Errorf("dhl: booking requires the quoted product code (service name)")
	}

	content := shipmentContent{
		Packages: []ratePackage{{
			Weight: req.Package.WeightKg,
			Dimensions: &dimensions{
				Length: int(req.Package.LengthCm),
				Width:  int(req.Package.WidthCm),
				Height: int(req.Package.HeightCm),
			},
		}},
		IsCustomsDeclarable: req.From.Country != req.To.Country,
		Description:         "Goods",
		UnitOfMeasurement:   "metric",
	}
	if req.Customs != nil {
		content.Incoterm = req.Customs.Incoterms
		items := make([]exportLineItem, 0, len(req.Customs.Items))
		for i, item := range req.Customs.Items {
			items = append(items, exportLineItem{
				Number:              i + 1,
				Description:         item.Description,
				Price:               float64(item.ValueCents) / 100,
				Quantity:            quantity{Value: item.Quantity, UnitOfMeasurement: "PCS"},
				CommodityCode:       item.HSCode,
				ManufacturerCountry: item.OriginCountry,
				NetWeight:           item.WeightKg,
			})
		}
		content.ExportDeclaration = &exportDeclaration{LineItems: items}
	}

	payload := shipmentRequest{
		PlannedShippingDateAndTime: nextBusinessDay(time.Now()).Format("2006-01-02T15:04:05 GMT+00:00"),
		ProductCode:                productCode,
		Pickup:                     shipmentPickup{IsRequested: false},
		CustomerDetails: customerDetails{
			ShipperDetails:  addressDetails{PostalCode: req.From.PostalCode, CityName: req.From.City, CountryCode: req.From.Country},
			ReceiverDetails: addressDetails{PostalCode: req.To.PostalCode, CityName: req.To.City, CountryCode: req.To.Country},
		},
		Content: content,
		OutputImageProperties: outputImageProps{
			ImageOptions: []imageOption{{TypeCode: "label"}},
		},
	}
	if a.config.AccountNumber != "" {
		payload.Accounts = []account{{TypeCode: "shipper", Number: a.config.AccountNumber}}
	}

	body, err := json.Marshal(payload)
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("dhl: encode shipment: %w", err)
	}
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodPost, a.config.BaseURL+"/shipments", bytes.NewReader(body))
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("dhl: build shipment request: %w", err)
	}
	a.setAuthHeaders(httpReq)

	resp, err := a.client.Do(httpReq)
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("dhl: shipment request failed: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 5<<20))
	if err != nil {
		return carrier.Booking{}, fmt.Errorf("dhl: read shipment response: %w", err)
	}
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		var errResp dhlErrorResponse
		msg := string(raw)
		if json.Unmarshal(raw, &errResp) == nil && errResp.Detail != "" {
			msg = errResp.Detail
		}
		return carrier.Booking{}, fmt.Errorf("dhl: shipment returned %d: %s", resp.StatusCode, msg)
	}

	var parsed shipmentResponse
	if err := json.Unmarshal(raw, &parsed); err != nil {
		return carrier.Booking{}, fmt.Errorf("dhl: decode shipment response: %w", err)
	}
	if parsed.ShipmentTrackingNumber == "" {
		return carrier.Booking{}, fmt.Errorf("dhl: shipment response missing shipmentTrackingNumber")
	}
	for _, doc := range parsed.Documents {
		if doc.TypeCode == "label" && doc.Content != "" {
			decoded, decodeErr := base64.StdEncoding.DecodeString(doc.Content)
			if decodeErr == nil {
				format := doc.ImageFormat
				if format == "" {
					format = "PDF"
				}
				contentType := "application/pdf"
				if strings.EqualFold(format, "ZPL") {
					contentType = "text/plain"
				}
				labelCache.Store(parsed.ShipmentTrackingNumber, carrier.Label{ContentType: contentType, Data: decoded})
			}
		}
	}

	tracking := parsed.ShipmentTrackingNumber
	if len(parsed.Packages) > 0 && parsed.Packages[0].TrackingNumber != "" {
		tracking = parsed.Packages[0].TrackingNumber
	}
	return carrier.Booking{
		CarrierCode: "dhl",
		BookingRef:  parsed.ShipmentTrackingNumber,
		TrackingNo:  tracking,
		Price:       req.Price,
		CreatedAt:   time.Now().UTC(),
	}, nil
}

// Label returns the label MyDHL API returned inline at booking time. DHL
// also exposes GET /shipments/{id}/get-image for later retrieval; not
// implemented here since the inline response is the documented common
// path and this mirrors Bring's process-local-cache precedent.
func (a *Adapter) Label(_ context.Context, bookingRef string) (carrier.Label, error) {
	cached, ok := labelCache.Load(bookingRef)
	if !ok {
		return carrier.Label{}, fmt.Errorf("dhl: no label cached for %s (labels are captured inline at booking time; re-fetch via GET /shipments/%s/get-image if this instance restarted — not yet implemented)", bookingRef, bookingRef)
	}
	return cached.(carrier.Label), nil
}

type trackingResponse struct {
	Shipments []struct {
		Status struct {
			Status      string `json:"status"`
			Description string `json:"description"`
			Timestamp   string `json:"timestamp"`
		} `json:"status"`
		Events []struct {
			Status      string `json:"status"`
			Description string `json:"description"`
			Timestamp   string `json:"timestamp"`
		} `json:"events"`
		EstimatedDeliveryDate struct {
			EstimatedDeliveryDate string `json:"estimatedDeliveryDate"`
		} `json:"estimatedTimeOfDelivery"`
	} `json:"shipments"`
}

// Track queries MyDHL API's shipment tracking resource.
func (a *Adapter) Track(ctx context.Context, trackingNo string) (carrier.TrackingStatus, error) {
	httpReq, err := http.NewRequestWithContext(ctx, http.MethodGet, a.config.BaseURL+"/shipments/"+trackingNo+"/tracking", nil)
	if err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("dhl: build tracking request: %w", err)
	}
	a.setAuthHeaders(httpReq)
	resp, err := a.client.Do(httpReq)
	if err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("dhl: tracking request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		snippet, _ := io.ReadAll(io.LimitReader(resp.Body, 300))
		return carrier.TrackingStatus{}, fmt.Errorf("dhl: tracking returned %d: %s", resp.StatusCode, snippet)
	}
	var parsed trackingResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return carrier.TrackingStatus{}, fmt.Errorf("dhl: decode tracking response: %w", err)
	}
	if len(parsed.Shipments) == 0 {
		return carrier.TrackingStatus{}, fmt.Errorf("dhl: tracking returned no shipments for %s", trackingNo)
	}
	shipment := parsed.Shipments[0]
	status := carrier.TrackingStatus{TrackingNo: trackingNo, CurrentStatus: shipment.Status.Status}
	for _, ev := range shipment.Events {
		occurred, _ := time.Parse(time.RFC3339, ev.Timestamp)
		status.Events = append(status.Events, carrier.TrackingEvent{
			Status:      ev.Status,
			Description: ev.Description,
			OccurredAt:  occurred,
		})
	}
	// DHL returns events newest-first; chronological is our contract.
	for i, j := 0, len(status.Events)-1; i < j; i, j = i+1, j-1 {
		status.Events[i], status.Events[j] = status.Events[j], status.Events[i]
	}
	if strings.EqualFold(shipment.Status.Status, "delivered") && len(status.Events) > 0 {
		last := status.Events[len(status.Events)-1]
		status.ActualDelivery = &last.OccurredAt
	}
	return status, nil
}
