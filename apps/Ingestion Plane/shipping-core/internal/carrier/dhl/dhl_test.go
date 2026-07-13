package dhl

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"shipping-core/internal/carrier"
)

func testRequest() carrier.QuoteRequest {
	return carrier.QuoteRequest{
		From:    carrier.Address{Name: "Velion AS", PostalCode: "0150", City: "Oslo", Country: "NO"},
		To:      carrier.Address{Name: "Empfänger GmbH", PostalCode: "10115", City: "Berlin", Country: "DE"},
		Package: carrier.Package{WeightKg: 5, LengthCm: 30, WidthCm: 20, HeightCm: 15},
		Segment: carrier.SegmentB2B,
	}
}

const exampleRateResponse = `{
  "products": [
    {
      "productCode": "P",
      "productName": "EXPRESS WORLDWIDE",
      "totalPrice": [{"price": "845.50", "priceCurrency": "NOK"}],
      "deliveryCapabilities": {"totalTransitDays": 2, "estimatedDeliveryDateAndTime": "2026-07-08T18:00:00Z"}
    },
    {
      "productCode": "U",
      "productName": "EXPRESS WORLDWIDE (NON-DOC)",
      "totalPrice": [{"price": 912.0, "priceCurrency": "NOK"}]
    }
  ]
}`

func TestAdapter_Info(t *testing.T) {
	info := New(Config{}).Info()
	if info.Code != "dhl" || info.Segment != carrier.SegmentB2B {
		t.Errorf("got %+v, want code=dhl segment=b2b", info)
	}
}

func TestAdapter_Quote_SendsBasicAuthAndParsesResponse(t *testing.T) {
	var gotUser, gotPass string
	var ok bool
	var gotBody rateRequest

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/rates" {
			t.Errorf("path = %q, want /rates", r.URL.Path)
		}
		gotUser, gotPass, ok = r.BasicAuth()
		if err := json.NewDecoder(r.Body).Decode(&gotBody); err != nil {
			t.Errorf("failed to decode request body: %v", err)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(exampleRateResponse))
	}))
	defer server.Close()

	a := New(Config{APIKey: "myKey", APISecret: "mySecret", AccountNumber: "123456789", BaseURL: server.URL})
	quotes, err := a.Quote(context.Background(), testRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if !ok || gotUser != "myKey" || gotPass != "mySecret" {
		t.Fatalf("Basic auth = ok=%v user=%q pass=%q, want myKey/mySecret", ok, gotUser, gotPass)
	}
	if len(gotBody.Accounts) != 1 || gotBody.Accounts[0].Number != "123456789" || gotBody.Accounts[0].TypeCode != "shipper" {
		t.Errorf("accounts = %+v", gotBody.Accounts)
	}
	if gotBody.ProductCode != "" {
		t.Errorf("productCode = %q, want empty (list all products)", gotBody.ProductCode)
	}
	if !gotBody.IsCustomsDeclarable {
		t.Errorf("isCustomsDeclarable = false, want true for NO->DE")
	}
	if gotBody.CustomerDetails.ShipperDetails.CountryCode != "NO" || gotBody.CustomerDetails.ReceiverDetails.CountryCode != "DE" {
		t.Errorf("shipper/receiver country codes wrong: %+v", gotBody.CustomerDetails)
	}

	if len(quotes) != 2 {
		t.Fatalf("got %d quotes, want 2", len(quotes))
	}
	if quotes[0].Price.AmountCents != 84550 || quotes[0].Price.Currency != "NOK" {
		t.Errorf("quotes[0].Price = %+v, want 84550 NOK", quotes[0].Price)
	}
	if quotes[0].TransitDays != 2 {
		t.Errorf("quotes[0].TransitDays = %d, want 2", quotes[0].TransitDays)
	}
	// Numeric (non-string) price in the response must parse too.
	if quotes[1].Price.AmountCents != 91200 {
		t.Errorf("quotes[1].Price.AmountCents = %d, want 91200 (numeric price field)", quotes[1].Price.AmountCents)
	}
}

func TestAdapter_Quote_DomesticShipmentIsNotCustomsDeclarable(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body rateRequest
		_ = json.NewDecoder(r.Body).Decode(&body)
		if body.IsCustomsDeclarable {
			t.Errorf("isCustomsDeclarable = true for a domestic NO->NO shipment, want false")
		}
		_, _ = w.Write([]byte(`{"products":[]}`))
	}))
	defer server.Close()

	req := testRequest()
	req.To.Country = "NO"
	a := New(Config{APIKey: "k", APISecret: "s", BaseURL: server.URL})
	if _, err := a.Quote(context.Background(), req); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestAdapter_Quote_UnexpectedStatusSurfacesDetail(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = w.Write([]byte(`{"status":"401","detail":"authorization header missing or invalid"}`))
	}))
	defer server.Close()

	a := New(Config{APIKey: "k", APISecret: "s", BaseURL: server.URL})
	_, err := a.Quote(context.Background(), testRequest())
	if err == nil {
		t.Fatal("expected an error for a 401 response")
	}
}

func testBookingRequest() carrier.BookingRequest {
	return carrier.BookingRequest{
		ServiceName: "P",
		Price:       carrier.Money{AmountCents: 84550, Currency: "NOK"},
		From:        carrier.Address{Name: "Velion AS", PostalCode: "0150", City: "Oslo", Country: "NO"},
		To:          carrier.Address{Name: "Empfänger GmbH", PostalCode: "10115", City: "Berlin", Country: "DE"},
		Package:     carrier.Package{WeightKg: 5, LengthCm: 30, WidthCm: 20, HeightCm: 15},
		Customs: &carrier.CustomsInfo{
			ContentsType: "goods",
			Incoterms:    "DAP",
			Items: []carrier.CustomsItem{
				{Description: "Widgets", Quantity: 2, ValueCents: 5000, Currency: "NOK", WeightKg: 1, HSCode: "8471.30", OriginCountry: "NO"},
			},
		},
	}
}

func TestAdapter_Book_BlockedAgainstNonSandboxBaseURLByDefault(t *testing.T) {
	a := New(Config{APIKey: "k", APISecret: "s", BaseURL: "https://express.api.dhl.com/mydhlapi"})
	if _, err := a.Book(context.Background(), testBookingRequest()); err == nil {
		t.Fatal("expected Book to refuse a non-sandbox BaseURL without LiveBooking")
	}
}

const exampleShipmentResponse = `{
  "shipmentTrackingNumber": "3245880253",
  "packages": [{"trackingNumber": "3245880253"}],
  "documents": [{"typeCode": "label", "imageFormat": "PDF", "content": "JVBERi0xLjQK"}]
}`

func TestAdapter_Book_SandboxURLSucceedsAndCachesLabel(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body shipmentRequest
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		if body.ProductCode != "P" {
			t.Errorf("productCode = %q, want %q", body.ProductCode, "P")
		}
		if !body.Content.IsCustomsDeclarable {
			t.Error("isCustomsDeclarable = false for a cross-border NO->DE shipment, want true")
		}
		_, _ = w.Write([]byte(exampleShipmentResponse))
	}))
	defer server.Close()

	a := New(Config{APIKey: "k", APISecret: "s", BaseURL: server.URL + "/test"})
	booking, err := a.Book(context.Background(), testBookingRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if booking.BookingRef != "3245880253" || booking.TrackingNo != "3245880253" {
		t.Errorf("unexpected booking: %+v", booking)
	}

	label, err := a.Label(context.Background(), booking.BookingRef)
	if err != nil {
		t.Fatalf("unexpected label error: %v", err)
	}
	if label.ContentType != "application/pdf" || len(label.Data) == 0 {
		t.Errorf("unexpected label: %+v", label)
	}
}

func TestAdapter_Book_LiveBookingAllowsProductionURL(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(exampleShipmentResponse))
	}))
	defer server.Close()

	a := New(Config{APIKey: "k", APISecret: "s", BaseURL: server.URL, LiveBooking: true})
	if _, err := a.Book(context.Background(), testBookingRequest()); err != nil {
		t.Fatalf("unexpected error with LiveBooking=true: %v", err)
	}
}

func TestAdapter_Label_NotCached(t *testing.T) {
	a := New(Config{APIKey: "k", APISecret: "s", BaseURL: "http://unused.invalid/test"})
	if _, err := a.Label(context.Background(), "never-booked"); err == nil {
		t.Fatal("expected an error for an uncached booking ref")
	}
}

const exampleTrackingResponse = `{
  "shipments": [{
    "status": {"status": "delivered", "description": "Delivered", "timestamp": "2026-07-08T14:00:00Z"},
    "events": [
      {"status": "delivered", "description": "Delivered", "timestamp": "2026-07-08T14:00:00Z"},
      {"status": "transit", "description": "Departed facility", "timestamp": "2026-07-07T09:00:00Z"}
    ]
  }]
}`

func TestAdapter_Track_ParsesChronologicalEventsAndDelivery(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			t.Errorf("method = %s, want GET", r.Method)
		}
		_, _ = w.Write([]byte(exampleTrackingResponse))
	}))
	defer server.Close()

	a := New(Config{APIKey: "k", APISecret: "s", BaseURL: server.URL + "/test"})
	status, err := a.Track(context.Background(), "3245880253")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if status.CurrentStatus != "delivered" {
		t.Errorf("CurrentStatus = %q, want delivered", status.CurrentStatus)
	}
	if len(status.Events) != 2 || status.Events[0].Description != "Departed facility" {
		t.Errorf("events not chronological: %+v", status.Events)
	}
	if status.ActualDelivery == nil {
		t.Error("expected ActualDelivery to be set for a delivered shipment")
	}
}
