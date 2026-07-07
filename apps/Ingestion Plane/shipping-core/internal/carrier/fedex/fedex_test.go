package fedex

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
		To:      carrier.Address{Name: "Recipient Inc", PostalCode: "10001", City: "New York", Country: "US"},
		Package: carrier.Package{WeightKg: 5, LengthCm: 30.4, WidthCm: 20, HeightCm: 15},
		Segment: carrier.SegmentB2B,
	}
}

const exampleResponse = `{
  "output": {
    "rateReplyDetails": [
      {
        "serviceType": "INTERNATIONAL_PRIORITY",
        "serviceName": "FedEx International Priority®",
        "ratedShipmentDetails": [
          {"rateType": "LIST", "totalNetCharge": 1450.00, "currency": "NOK"},
          {"rateType": "ACCOUNT", "totalNetCharge": 1201.50, "currency": "NOK"}
        ],
        "commit": {"transitDays": {"minimumTransitTime": "TWO_DAYS"}}
      },
      {
        "serviceType": "INTERNATIONAL_ECONOMY",
        "serviceName": "FedEx International Economy®",
        "ratedShipmentDetails": [
          {"rateType": "ACCOUNT", "totalNetCharge": 890.00, "currency": "NOK"}
        ]
      }
    ]
  }
}`

// newTestServer stands in for both the FedEx OAuth token endpoint and the
// Rate endpoint.
func newTestServer(t *testing.T, rateHandler http.HandlerFunc) *httptest.Server {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/oauth/token", func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseForm(); err != nil {
			t.Errorf("token request: parse form: %v", err)
		}
		if r.PostForm.Get("grant_type") != "client_credentials" {
			t.Errorf("token request: grant_type = %q, want client_credentials", r.PostForm.Get("grant_type"))
		}
		if r.PostForm.Get("client_id") != "test-client-id" || r.PostForm.Get("client_secret") != "test-secret" {
			t.Errorf("token request: credentials not sent as form params (FedEx auth style)")
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"access_token":"test-token","token_type":"bearer","expires_in":3600}`))
	})
	mux.HandleFunc("/rate/v1/rates/quotes", rateHandler)
	return httptest.NewServer(mux)
}

func newTestAdapter(server *httptest.Server) *Adapter {
	return New(Config{
		ClientID:      "test-client-id",
		ClientSecret:  "test-secret",
		AccountNumber: "740561073",
		BaseURL:       server.URL,
	})
}

func TestAdapter_Info(t *testing.T) {
	a := New(Config{})
	info := a.Info()
	if info.Code != "fedex" || info.Segment != carrier.SegmentB2B {
		t.Errorf("got %+v, want code=fedex segment=b2b", info)
	}
}

func TestAdapter_Quote_SendsBearerTokenAndMappedBody(t *testing.T) {
	var gotAuth string
	var gotBody rateRequest

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
	if gotBody.AccountNumber.Value != "740561073" {
		t.Errorf("accountNumber = %q, want 740561073", gotBody.AccountNumber.Value)
	}
	ship := gotBody.RequestedShipment
	if ship.Shipper.Address.PostalCode != "0150" || ship.Recipient.Address.CountryCode != "US" {
		t.Errorf("addresses = %+v -> %+v, want 0150/NO -> US", ship.Shipper.Address, ship.Recipient.Address)
	}
	if len(ship.RequestedPackageLineItems) != 1 {
		t.Fatalf("got %d package lines, want 1", len(ship.RequestedPackageLineItems))
	}
	line := ship.RequestedPackageLineItems[0]
	if line.Weight.Units != "KG" || line.Weight.Value != 5 {
		t.Errorf("weight = %+v, want 5 KG", line.Weight)
	}
	if line.Dimensions.Length != 31 {
		t.Errorf("length = %d, want 31 (30.4 rounded up — FedEx requires integers)", line.Dimensions.Length)
	}
}

func TestAdapter_Quote_ParsesResponsePreferringAccountRate(t *testing.T) {
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

	priority := quotes[0]
	if priority.ServiceName != "FedEx International Priority®" {
		t.Errorf("ServiceName = %q", priority.ServiceName)
	}
	// ACCOUNT rate (1201.50), not LIST (1450.00), even though LIST comes first.
	if priority.Price.AmountCents != 120150 || priority.Price.Currency != "NOK" {
		t.Errorf("Price = %+v, want 120150 NOK (ACCOUNT rate preferred)", priority.Price)
	}
	if priority.TransitDays != 2 {
		t.Errorf("TransitDays = %d, want 2 (TWO_DAYS)", priority.TransitDays)
	}

	economy := quotes[1]
	if economy.Price.AmountCents != 89000 {
		t.Errorf("Price = %+v, want 89000", economy.Price)
	}
	if economy.TransitDays != 0 {
		t.Errorf("TransitDays = %d, want 0 (no commit info in response)", economy.TransitDays)
	}
}

func TestAdapter_Quote_ErrorStatuses(t *testing.T) {
	tests := []struct {
		name   string
		status int
	}{
		{"rate limited", http.StatusTooManyRequests},
		{"forbidden", http.StatusForbidden},
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
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer server.Close()

	a := New(Config{ClientID: "bad", ClientSecret: "bad", AccountNumber: "x", BaseURL: server.URL})
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

func TestAdapter_BookLabelTrack_NotImplemented(t *testing.T) {
	a := New(Config{})
	if _, err := a.Book(context.Background(), carrier.BookingRequest{}); err == nil {
		t.Error("expected Book to return an error in this phase")
	}
	if _, err := a.Label(context.Background(), "ref"); err == nil {
		t.Error("expected Label to return an error in this phase")
	}
	if _, err := a.Track(context.Background(), "no"); err == nil {
		t.Error("expected Track to return an error in this phase")
	}
}
