package bring

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
		From:    carrier.Address{PostalCode: "0150", Country: "NO"},
		To:      carrier.Address{PostalCode: "7010", Country: "NO"},
		Package: carrier.Package{WeightKg: 5, LengthCm: 30, WidthCm: 20, HeightCm: 15},
		Segment: carrier.SegmentB2B,
	}
}

// exampleResponse mirrors the confirmed response shape from
// developer.bring.com's published "Fetch shipping details" examples.
const exampleResponse = `{
  "consignments": [
    {
      "consignmentId": "1",
      "products": [
        {
          "id": "5600",
          "productionCode": "5600",
          "price": {
            "listPrice": {
              "currencyCode": "NOK",
              "priceWithAdditionalServices": {
                "amountWithVAT": "285.96"
              }
            }
          },
          "expectedDelivery": {
            "alternativeDeliveryDates": [
              {
                "workingDays": "2",
                "formattedExpectedDeliveryDate": "18.11.2026"
              }
            ]
          }
        }
      ]
    }
  ],
  "uniqueId": "66091c94-6565-4c42-9c49-34bab012236d"
}`

func TestAdapter_Info(t *testing.T) {
	a := New(Config{})
	info := a.Info()
	if info.Code != "bring" || info.Segment != carrier.SegmentBoth {
		t.Errorf("got %+v, want code=bring segment=both", info)
	}
}

func TestAdapter_Quote_SendsExpectedAuthHeadersAndBody(t *testing.T) {
	var gotUID, gotKey, gotClientURL string
	var gotBody consignmentRequest

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotUID = r.Header.Get("X-Mybring-API-Uid")
		gotKey = r.Header.Get("X-Mybring-API-Key")
		gotClientURL = r.Header.Get("X-Bring-Client-URL")

		var decoded shippingGuideRequest
		if err := json.NewDecoder(r.Body).Decode(&decoded); err != nil {
			t.Errorf("failed to decode request body: %v", err)
		}
		if len(decoded.Consignments) != 1 {
			t.Fatalf("got %d consignments, want 1", len(decoded.Consignments))
		}
		gotBody = decoded.Consignments[0]

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(exampleResponse))
	}))
	defer server.Close()

	a := New(Config{APIUID: "user@example.com", APIKey: "secret-key", CustomerNumber: "5", ClientURL: "https://shop.example.com", BaseURL: server.URL})
	_, err := a.Quote(context.Background(), testRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if gotUID != "user@example.com" {
		t.Errorf("X-Mybring-API-Uid = %q, want %q", gotUID, "user@example.com")
	}
	if gotKey != "secret-key" {
		t.Errorf("X-Mybring-API-Key = %q, want %q", gotKey, "secret-key")
	}
	// developer.bring.com/api documents X-Bring-Client-URL as always required
	// alongside the two headers above — Quote used to omit it (only
	// Book/Label/Track set it).
	if gotClientURL != "https://shop.example.com" {
		t.Errorf("X-Bring-Client-URL = %q, want %q", gotClientURL, "https://shop.example.com")
	}
	if len(gotBody.Products) == 0 {
		t.Fatal("expected at least one requested product")
	}
	for _, p := range gotBody.Products {
		if p.CustomerNumber != "5" {
			t.Errorf("product %q customerNumber = %q, want %q", p.ID, p.CustomerNumber, "5")
		}
	}
	if gotBody.FromPostalCode != "0150" || gotBody.ToPostalCode != "7010" {
		t.Errorf("postal codes = %q -> %q, want 0150 -> 7010", gotBody.FromPostalCode, gotBody.ToPostalCode)
	}
	// weight converts kg -> grams; dimensions pass through unchanged (cm).
	if len(gotBody.Packages) != 1 || gotBody.Packages[0].GrossWeight != 5000 {
		t.Errorf("packages = %+v, want one package at 5000g", gotBody.Packages)
	}
	if gotBody.Packages[0].Height != 15 || gotBody.Packages[0].Width != 20 || gotBody.Packages[0].Length != 30 {
		t.Errorf("package dimensions = %+v, want 15x20x30cm", gotBody.Packages[0])
	}
}

func TestAdapter_Quote_ParsesResponseIntoDomainQuote(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(exampleResponse))
	}))
	defer server.Close()

	a := New(Config{BaseURL: server.URL})
	quotes, err := a.Quote(context.Background(), testRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(quotes) != 1 {
		t.Fatalf("got %d quotes, want 1", len(quotes))
	}

	q := quotes[0]
	if q.CarrierCode != "bring" {
		t.Errorf("CarrierCode = %q, want bring", q.CarrierCode)
	}
	if q.Price.AmountCents != 28596 || q.Price.Currency != "NOK" {
		t.Errorf("Price = %+v, want 28596 NOK", q.Price)
	}
	if q.TransitDays != 2 {
		t.Errorf("TransitDays = %d, want 2", q.TransitDays)
	}
	wantDate := time.Date(2026, 11, 18, 0, 0, 0, 0, time.UTC)
	if !q.EstimatedDelivery.Equal(wantDate) {
		t.Errorf("EstimatedDelivery = %v, want %v", q.EstimatedDelivery, wantDate)
	}
}

// productionShapedResponse mirrors the delivery-time shape Bring's live
// Shipping Guide 2.0 actually returns (captured 2026-07-10, Oslo 0150 ->
// Trondheim 7010): the promise sits at the TOP level of expectedDelivery
// (workingDays + formattedExpectedDeliveryDate + structured
// expectedDeliveryDate) with an EMPTY alternativeDeliveryDates array. The
// older fixture above nested everything under alternativeDeliveryDates, which
// is why the parse test passed green while live quotes silently returned
// TransitDays:0 / 0001-01-01. This test guards the regression.
const productionShapedResponse = `{
  "consignments": [
    {
      "consignmentId": "1",
      "products": [
        {
          "id": "9300",
          "productionCode": "9300",
          "price": {
            "listPrice": {
              "currencyCode": "NOK",
              "priceWithAdditionalServices": {
                "amountWithVAT": "272.73"
              }
            }
          },
          "expectedDelivery": {
            "workingDays": "2",
            "formattedExpectedDeliveryDate": "14.07.2026",
            "expectedDeliveryDate": {"year": "2026", "month": "7", "day": "14"},
            "alternativeDeliveryDates": []
          }
        }
      ]
    }
  ],
  "uniqueId": "b0f4b0a2-0000-4000-8000-000000000000"
}`

func TestAdapter_Quote_ParsesTopLevelExpectedDelivery(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(productionShapedResponse))
	}))
	defer server.Close()

	a := New(Config{BaseURL: server.URL})
	quotes, err := a.Quote(context.Background(), testRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(quotes) != 1 {
		t.Fatalf("got %d quotes, want 1", len(quotes))
	}

	q := quotes[0]
	// The whole point of the fix: a top-level promise must NOT be dropped.
	if q.TransitDays != 2 {
		t.Errorf("TransitDays = %d, want 2 (top-level workingDays must not be dropped)", q.TransitDays)
	}
	wantDate := time.Date(2026, 7, 14, 0, 0, 0, 0, time.UTC)
	if !q.EstimatedDelivery.Equal(wantDate) {
		t.Errorf("EstimatedDelivery = %v, want %v (top-level date must not be dropped)", q.EstimatedDelivery, wantDate)
	}
}

// TestAdapter_Quote_FallsBackToStructuredDate proves the structured
// expectedDeliveryDate object is used when the formatted string is localized
// (Bring sometimes returns e.g. "tirsdag 14. juli") and thus unparseable by
// the "02.01.2006" layout.
func TestAdapter_Quote_FallsBackToStructuredDate(t *testing.T) {
	const localizedResponse = `{
  "consignments": [{"consignmentId": "1", "products": [{
    "id": "9300", "productionCode": "9300",
    "price": {"listPrice": {"currencyCode": "NOK", "priceWithAdditionalServices": {"amountWithVAT": "272.73"}}},
    "expectedDelivery": {
      "workingDays": "2",
      "formattedExpectedDeliveryDate": "tirsdag 14. juli 2026",
      "expectedDeliveryDate": {"year": "2026", "month": "7", "day": "14"},
      "alternativeDeliveryDates": []
    }
  }]}],
  "uniqueId": "b0f4b0a2-0000-4000-8000-000000000001"
}`
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(localizedResponse))
	}))
	defer server.Close()

	a := New(Config{BaseURL: server.URL})
	quotes, err := a.Quote(context.Background(), testRequest())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(quotes) != 1 {
		t.Fatalf("got %d quotes, want 1", len(quotes))
	}
	q := quotes[0]
	if q.TransitDays != 2 {
		t.Errorf("TransitDays = %d, want 2", q.TransitDays)
	}
	wantDate := time.Date(2026, 7, 14, 0, 0, 0, 0, time.UTC)
	if !q.EstimatedDelivery.Equal(wantDate) {
		t.Errorf("EstimatedDelivery = %v, want %v (structured date fallback)", q.EstimatedDelivery, wantDate)
	}
}

func TestAdapter_Quote_RateLimited(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusTooManyRequests)
	}))
	defer server.Close()

	a := New(Config{BaseURL: server.URL})
	_, err := a.Quote(context.Background(), testRequest())
	if err == nil {
		t.Fatal("expected an error on 429, got nil")
	}
}

func TestAdapter_Quote_UnexpectedStatus(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer server.Close()

	a := New(Config{BaseURL: server.URL})
	_, err := a.Quote(context.Background(), testRequest())
	if err == nil {
		t.Fatal("expected an error on 500, got nil")
	}
}

func TestAdapter_Quote_MalformedJSON(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte("not json"))
	}))
	defer server.Close()

	a := New(Config{BaseURL: server.URL})
	_, err := a.Quote(context.Background(), testRequest())
	if err == nil {
		t.Fatal("expected an error on malformed JSON, got nil")
	}
}

func TestAdapter_Quote_RespectsContextCancellation(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		time.Sleep(100 * time.Millisecond)
		w.Write([]byte(exampleResponse))
	}))
	defer server.Close()

	a := New(Config{BaseURL: server.URL})
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()

	_, err := a.Quote(ctx, testRequest())
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
	if _, err := a.Track(context.Background(), "trackingno"); err == nil {
		t.Error("expected Track to return an error in this phase")
	}
}
