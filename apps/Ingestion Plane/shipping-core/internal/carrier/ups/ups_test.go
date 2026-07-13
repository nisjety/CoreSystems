package ups

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"shipping-core/internal/carrier"
)

func testRequest() carrier.QuoteRequest {
	return carrier.QuoteRequest{
		From:    carrier.Address{Name: "Sender AS", PostalCode: "0150", City: "Oslo", Country: "NO"},
		To:      carrier.Address{Name: "Mottaker AS", PostalCode: "20095", City: "Hamburg", Country: "DE"},
		Package: carrier.Package{WeightKg: 5, LengthCm: 30, WidthCm: 20, HeightCm: 15},
		Segment: carrier.SegmentB2B,
	}
}

const exampleResponse = `{
  "RateResponse": {
    "Response": {"ResponseStatus": {"Code": "1", "Description": "Success"}},
    "RatedShipment": [
      {
        "Service": {"Code": "11"},
        "TotalCharges": {"CurrencyCode": "NOK", "MonetaryValue": "412.50"},
        "GuaranteedDelivery": {"BusinessDaysInTransit": "3"}
      },
      {
        "Service": {"Code": "65"},
        "TotalCharges": {"CurrencyCode": "NOK", "MonetaryValue": "689.00"}
      }
    ]
  }
}`

// newTestServer stands in for both the UPS OAuth token endpoint and the
// Rating endpoint, since the adapter's HTTP client fetches a token
// transparently before the first rating call.
func newTestServer(t *testing.T, rateHandler http.HandlerFunc) *httptest.Server {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/security/v1/oauth/token", func(w http.ResponseWriter, r *http.Request) {
		user, pass, ok := r.BasicAuth()
		if !ok || user != "test-client-id" || pass != "test-secret" {
			t.Errorf("token request: expected Basic auth with client credentials, got ok=%v user=%q", ok, user)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"access_token":"test-token","token_type":"Bearer","expires_in":3600}`))
	})
	mux.HandleFunc("/api/rating/v2409/Shop", rateHandler)
	return httptest.NewServer(mux)
}

func newTestAdapter(server *httptest.Server) *Adapter {
	return New(Config{
		ClientID:      "test-client-id",
		ClientSecret:  "test-secret",
		AccountNumber: "A1B2C3",
		BaseURL:       server.URL,
	})
}

func TestAdapter_Info(t *testing.T) {
	a := New(Config{})
	info := a.Info()
	if info.Code != "ups" || info.Segment != carrier.SegmentB2B {
		t.Errorf("got %+v, want code=ups segment=b2b", info)
	}
}

func TestAdapter_Quote_SendsBearerTokenAndMappedBody(t *testing.T) {
	var gotAuth string
	var gotBody rateRequestEnvelope

	server := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		gotAuth = r.Header.Get("Authorization")
		if err := json.NewDecoder(r.Body).Decode(&gotBody); err != nil {
			t.Errorf("decode request body: %v", err)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(exampleResponse))
	})
	defer server.Close()

	_, err := newTestAdapter(server).Quote(context.Background(), testRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if gotAuth != "Bearer test-token" {
		t.Errorf("Authorization = %q, want Bearer test-token", gotAuth)
	}
	shipment := gotBody.RateRequest.Shipment
	if shipment.Shipper.ShipperNumber != "A1B2C3" {
		t.Errorf("ShipperNumber = %q, want A1B2C3", shipment.Shipper.ShipperNumber)
	}
	if shipment.ShipFrom.Address.PostalCode != "0150" || shipment.ShipTo.Address.PostalCode != "20095" {
		t.Errorf("postal codes = %q -> %q, want 0150 -> 20095",
			shipment.ShipFrom.Address.PostalCode, shipment.ShipTo.Address.PostalCode)
	}
	if len(shipment.Package) != 1 {
		t.Fatalf("got %d packages, want 1", len(shipment.Package))
	}
	pkg := shipment.Package[0]
	if pkg.PackageWeight.Weight != "5" || pkg.PackageWeight.UnitOfMeasurement.Code != "KGS" {
		t.Errorf("weight = %+v, want 5 KGS", pkg.PackageWeight)
	}
	if pkg.Dimensions.UnitOfMeasurement.Code != "CM" || pkg.Dimensions.Length != "30.0" {
		t.Errorf("dimensions = %+v, want CM 30.0", pkg.Dimensions)
	}
}

func TestAdapter_Quote_ParsesResponseIntoDomainQuotes(t *testing.T) {
	server := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(exampleResponse))
	})
	defer server.Close()

	quotes, err := newTestAdapter(server).Quote(context.Background(), testRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(quotes) != 2 {
		t.Fatalf("got %d quotes, want 2", len(quotes))
	}

	standard := quotes[0]
	if standard.ServiceName != "UPS Standard" {
		t.Errorf("ServiceName = %q, want UPS Standard (code 11 mapped)", standard.ServiceName)
	}
	if standard.Price.AmountCents != 41250 || standard.Price.Currency != "NOK" {
		t.Errorf("Price = %+v, want 41250 NOK", standard.Price)
	}
	if standard.TransitDays != 3 {
		t.Errorf("TransitDays = %d, want 3", standard.TransitDays)
	}

	saver := quotes[1]
	if saver.ServiceName != "UPS Express Saver" {
		t.Errorf("ServiceName = %q, want UPS Express Saver (code 65 mapped)", saver.ServiceName)
	}
	if saver.TransitDays != 0 {
		t.Errorf("TransitDays = %d, want 0 (no GuaranteedDelivery in response)", saver.TransitDays)
	}
}

func TestAdapter_Quote_UnknownServiceCodeFallsBack(t *testing.T) {
	server := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"RateResponse":{"RatedShipment":[{"Service":{"Code":"99"},"TotalCharges":{"CurrencyCode":"NOK","MonetaryValue":"1.00"}}]}}`))
	})
	defer server.Close()

	quotes, err := newTestAdapter(server).Quote(context.Background(), testRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if quotes[0].ServiceName != "UPS 99" {
		t.Errorf("ServiceName = %q, want fallback UPS 99", quotes[0].ServiceName)
	}
}

func TestAdapter_Quote_ErrorStatuses(t *testing.T) {
	tests := []struct {
		name   string
		status int
	}{
		{"rate limited", http.StatusTooManyRequests},
		{"unauthorized", http.StatusUnauthorized},
		{"server error", http.StatusInternalServerError},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			server := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
				w.WriteHeader(tt.status)
			})
			defer server.Close()

			_, err := newTestAdapter(server).Quote(context.Background(), testRequest())
			if err == nil {
				t.Fatalf("expected an error on status %d, got nil", tt.status)
			}
		})
	}
}

func TestAdapter_Quote_TokenEndpointFailure(t *testing.T) {
	// A server that fails the token request itself — the adapter should
	// surface this as an error, not panic or hang.
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer server.Close()

	a := New(Config{ClientID: "bad", ClientSecret: "bad", BaseURL: server.URL})
	_, err := a.Quote(context.Background(), testRequest())
	if err == nil {
		t.Fatal("expected an error when the token endpoint rejects credentials")
	}
}

func TestAdapter_Quote_MalformedJSON(t *testing.T) {
	server := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte("not json"))
	})
	defer server.Close()

	_, err := newTestAdapter(server).Quote(context.Background(), testRequest())
	if err == nil {
		t.Fatal("expected an error on malformed JSON")
	}
}

func TestAdapter_Quote_RespectsContextCancellation(t *testing.T) {
	server := newTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		time.Sleep(100 * time.Millisecond)
		_, _ = w.Write([]byte(exampleResponse))
	})
	defer server.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()

	_, err := newTestAdapter(server).Quote(ctx, testRequest())
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Errorf("got err=%v, want context.DeadlineExceeded", err)
	}
}

func newBookingTestServer(t *testing.T, shipHandler, trackHandler http.HandlerFunc) *httptest.Server {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/security/v1/oauth/token", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"access_token":"test-token","token_type":"Bearer","expires_in":3600}`))
	})
	if shipHandler != nil {
		mux.HandleFunc(shipPath, shipHandler)
	}
	if trackHandler != nil {
		mux.HandleFunc(trackPath, trackHandler)
	}
	return httptest.NewServer(mux)
}

func testBookingRequest() carrier.BookingRequest {
	return carrier.BookingRequest{
		ServiceName: "11",
		Price:       carrier.Money{AmountCents: 41250, Currency: "NOK"},
		From:        carrier.Address{Name: "Sender AS", PostalCode: "0150", City: "Oslo", Country: "NO"},
		To:          carrier.Address{Name: "Mottaker AS", PostalCode: "20095", City: "Hamburg", Country: "DE"},
		Package:     carrier.Package{WeightKg: 5, LengthCm: 30, WidthCm: 20, HeightCm: 15},
	}
}

func TestAdapter_Book_BlockedAgainstNonCIEBaseURLByDefault(t *testing.T) {
	a := New(Config{ClientID: "id", ClientSecret: "secret", BaseURL: "https://onlinetools.ups.com"})
	if _, err := a.Book(context.Background(), testBookingRequest()); err == nil {
		t.Fatal("expected Book to refuse a non-CIE BaseURL without LiveBooking")
	}
}

const exampleShipmentResponse = `{
  "ShipmentResponse": {
    "ShipmentResults": {
      "ShipmentIdentificationNumber": "1Z12345E0205271688",
      "PackageResults": [{"TrackingNumber": "1Z12345E0205271688", "ShippingLabel": {"ImageFormat": {"Code": "GIF"}, "GraphicImage": "R0lGODlhAQABAAAAACw="}}]
    }
  }
}`

func TestAdapter_Book_CIEURLSucceedsAndCachesLabel(t *testing.T) {
	server := newBookingTestServer(t, func(w http.ResponseWriter, r *http.Request) {
		var body shipmentRequestEnvelope
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		if body.ShipmentRequest.Shipment.Service.Code != "11" {
			t.Errorf("Service.Code = %q, want 11", body.ShipmentRequest.Shipment.Service.Code)
		}
		_, _ = w.Write([]byte(exampleShipmentResponse))
	}, nil)
	defer server.Close()

	// substring "wwwcie" must appear in BaseURL for the default-safe gate;
	// httptest gives us 127.0.0.1, so exercise the LiveBooking=true path
	// here and the substring gate is covered by the Blocked test above.
	a := New(Config{ClientID: "id", ClientSecret: "secret", BaseURL: server.URL, LiveBooking: true})
	booking, err := a.Book(context.Background(), testBookingRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if booking.BookingRef != "1Z12345E0205271688" || booking.TrackingNo != "1Z12345E0205271688" {
		t.Errorf("unexpected booking: %+v", booking)
	}

	label, err := a.Label(context.Background(), booking.TrackingNo)
	if err != nil {
		t.Fatalf("unexpected label error: %v", err)
	}
	if label.ContentType != "image/gif" || len(label.Data) == 0 {
		t.Errorf("unexpected label: %+v", label)
	}
}

func TestAdapter_Label_NotCached(t *testing.T) {
	a := New(Config{ClientID: "id", ClientSecret: "secret", BaseURL: "http://unused.invalid"})
	if _, err := a.Label(context.Background(), "never-booked"); err == nil {
		t.Fatal("expected an error for an uncached booking ref")
	}
}

const exampleTrackResponse = `{
  "trackResponse": {
    "shipment": [{
      "package": [{
        "trackingNumber": "1Z12345E0205271688",
        "activity": [
          {"status": {"type": "D", "description": "Delivered", "code": "KB"}, "date": "20260708", "time": "140000"},
          {"status": {"type": "I", "description": "Departed facility"}, "date": "20260707", "time": "090000"}
        ]
      }]
    }]
  }
}`

func TestAdapter_Track_ParsesChronologicalEventsAndDelivery(t *testing.T) {
	server := newBookingTestServer(t, nil, func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			t.Errorf("method = %s, want GET", r.Method)
		}
		_, _ = w.Write([]byte(exampleTrackResponse))
	})
	defer server.Close()

	a := New(Config{ClientID: "id", ClientSecret: "secret", BaseURL: server.URL})
	status, err := a.Track(context.Background(), "1Z12345E0205271688")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if status.CurrentStatus != "Delivered" {
		t.Errorf("CurrentStatus = %q, want Delivered", status.CurrentStatus)
	}
	if len(status.Events) != 2 || status.Events[0].Description != "Departed facility" {
		t.Errorf("events not chronological: %+v", status.Events)
	}
	if status.ActualDelivery == nil {
		t.Error("expected ActualDelivery to be set for a delivered shipment")
	}
}
