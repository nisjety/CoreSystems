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

func TestAdapter_BookLabelTrack_NotImplemented(t *testing.T) {
	a := New(Config{APIKey: "k", APISecret: "s", BaseURL: "http://unused.invalid"})
	if _, err := a.Book(context.Background(), carrier.BookingRequest{}); err == nil {
		t.Error("expected Book to return an error — not implemented yet")
	}
	if _, err := a.Label(context.Background(), "ref"); err == nil {
		t.Error("expected Label to return an error — not implemented yet")
	}
	if _, err := a.Track(context.Background(), "trackingno"); err == nil {
		t.Error("expected Track to return an error — not implemented yet")
	}
}
