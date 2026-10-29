package bring

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"shipping-core/internal/carrier"
)

func bookingRequestFixture() carrier.BookingRequest {
	return carrier.BookingRequest{
		QuoteRef:    "q1",
		ServiceName: "SERVICEPAKKE",
		Price:       carrier.Money{AmountCents: 18500, Currency: "NOK"},
		From:        carrier.Address{Name: "Verevon AS", Street: "Storgata 1", PostalCode: "0150", City: "Oslo", Country: "NO", IsBusiness: true},
		To:          carrier.Address{Name: "Kari Nordmann", Street: "Munkegata 2", PostalCode: "7010", City: "Trondheim", Country: "NO"},
		Package:     carrier.Package{WeightKg: 8, LengthCm: 40, WidthCm: 30, HeightCm: 20},
		BookedBy:    "test",
	}
}

func TestBookSendsTestIndicatorAndAuthAndMapsConfirmation(t *testing.T) {
	var gotBody map[string]any
	var gotUID, gotKey, gotClientURL string
	labelServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/pdf")
		_, _ = w.Write([]byte("%PDF-1.4 label"))
	}))
	defer labelServer.Close()

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotUID = r.Header.Get("X-Mybring-API-Uid")
		gotKey = r.Header.Get("X-Mybring-API-Key")
		gotClientURL = r.Header.Get("X-Bring-Client-URL")
		raw, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(raw, &gotBody)
		_, _ = w.Write([]byte(`{"consignments":[{"confirmation":{"consignmentNumber":"70438100000000001","links":{"labels":"` + labelServer.URL + `/label"},"packages":[{"packageNumber":"370438100000000002"}]}}]}`))
	}))
	defer srv.Close()

	// ClientURL left unset — must fall back to the package default rather
	// than sending an empty header.
	a := New(Config{APIUID: "uid@verevon.no", APIKey: "key123", CustomerNumber: "12345", BookingBaseURL: srv.URL})
	booked, err := a.Book(context.Background(), bookingRequestFixture())
	if err != nil {
		t.Fatalf("Book: %v", err)
	}

	if gotUID != "uid@verevon.no" || gotKey != "key123" {
		t.Fatalf("Mybring auth headers missing: uid=%q key=%q", gotUID, gotKey)
	}
	if gotClientURL == "" {
		t.Fatalf("X-Bring-Client-URL must never be sent empty — want the package default")
	}
	// testIndicator MUST default to true — a misconfigured environment must
	// never place a live freight order.
	if gotBody["testIndicator"] != true {
		t.Fatalf("testIndicator = %v, want true by default", gotBody["testIndicator"])
	}
	if booked.BookingRef != "70438100000000001" {
		t.Fatalf("booking ref = %q", booked.BookingRef)
	}
	if booked.TrackingNo != "370438100000000002" {
		t.Fatalf("tracking no = %q (want the package number)", booked.TrackingNo)
	}

	// Label link cached at booking time → Label() fetches the PDF.
	label, err := a.Label(context.Background(), booked.BookingRef)
	if err != nil {
		t.Fatalf("Label: %v", err)
	}
	if label.ContentType != "application/pdf" || len(label.Data) == 0 {
		t.Fatalf("label = %q (%d bytes)", label.ContentType, len(label.Data))
	}
}

func TestBookSendsRecipientContactWhenAddressHasPhoneOrEmail(t *testing.T) {
	var gotBody map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(raw, &gotBody)
		_, _ = w.Write([]byte(`{"consignments":[{"confirmation":{"consignmentNumber":"1","links":{},"packages":[{"packageNumber":"1"}]}}]}`))
	}))
	defer srv.Close()

	a := New(Config{APIUID: "u", APIKey: "k", CustomerNumber: "1", BookingBaseURL: srv.URL})
	req := bookingRequestFixture()
	req.To.Phone = "+4712345678"
	req.To.Email = "kari@example.no"
	if _, err := a.Book(context.Background(), req); err != nil {
		t.Fatalf("Book: %v", err)
	}

	consignments, _ := gotBody["consignments"].([]any)
	parties, _ := consignments[0].(map[string]any)["parties"].(map[string]any)
	recipient, _ := parties["recipient"].(map[string]any)
	contact, _ := recipient["contact"].(map[string]any)
	if contact["email"] != "kari@example.no" || contact["phoneNumber"] != "+4712345678" {
		t.Fatalf("recipient.contact = %+v, want email/phoneNumber populated", contact)
	}
	if contact["name"] != "Kari Nordmann" {
		t.Fatalf("recipient.contact.name = %v, want the party name", contact["name"])
	}
}

func TestBookOmitsContactWhenAddressHasNeitherPhoneNorEmail(t *testing.T) {
	var gotBody map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(raw, &gotBody)
		_, _ = w.Write([]byte(`{"consignments":[{"confirmation":{"consignmentNumber":"1","links":{},"packages":[{"packageNumber":"1"}]}}]}`))
	}))
	defer srv.Close()

	a := New(Config{APIUID: "u", APIKey: "k", CustomerNumber: "1", BookingBaseURL: srv.URL})
	if _, err := a.Book(context.Background(), bookingRequestFixture()); err != nil {
		t.Fatalf("Book: %v", err)
	}

	consignments, _ := gotBody["consignments"].([]any)
	parties, _ := consignments[0].(map[string]any)["parties"].(map[string]any)
	recipient, _ := parties["recipient"].(map[string]any)
	if _, present := recipient["contact"]; present {
		t.Fatalf("recipient.contact = %v, want omitted when the address has no phone/email", recipient["contact"])
	}
}

func TestBookingAllowedRequiresLiveBookingAndCustomerNumber(t *testing.T) {
	cases := []struct {
		name           string
		liveBooking    bool
		customerNumber string
		want           bool
	}{
		{"neither set", false, "", false},
		{"customer number only", false, "12345", false},
		{"live booking only — misconfigured, must stay in test mode", true, "", false},
		{"both set", true, "12345", true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a := New(Config{LiveBooking: tc.liveBooking, CustomerNumber: tc.customerNumber})
			if got := a.bookingAllowed(); got != tc.want {
				t.Fatalf("bookingAllowed() = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestBookStaysInTestModeWhenLiveBookingSetButCustomerNumberMissing(t *testing.T) {
	var gotBody map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(raw, &gotBody)
		_, _ = w.Write([]byte(`{"consignments":[{"confirmation":{"consignmentNumber":"1","links":{},"packages":[{"packageNumber":"1"}]}}]}`))
	}))
	defer srv.Close()

	// LiveBooking is set, but CustomerNumber is not — this must not be
	// enough to place a live freight order.
	a := New(Config{APIUID: "u", APIKey: "k", LiveBooking: true, BookingBaseURL: srv.URL})
	if _, err := a.Book(context.Background(), bookingRequestFixture()); err != nil {
		t.Fatalf("Book: %v", err)
	}
	if gotBody["testIndicator"] != true {
		t.Fatalf("testIndicator = %v, want true when CustomerNumber is missing even with LiveBooking set", gotBody["testIndicator"])
	}
}

func TestBookGoesLiveOnlyWhenLiveBookingAndCustomerNumberAreBothSet(t *testing.T) {
	var gotBody map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(raw, &gotBody)
		_, _ = w.Write([]byte(`{"consignments":[{"confirmation":{"consignmentNumber":"1","links":{},"packages":[{"packageNumber":"1"}]}}]}`))
	}))
	defer srv.Close()

	a := New(Config{APIUID: "u", APIKey: "k", LiveBooking: true, CustomerNumber: "12345", BookingBaseURL: srv.URL})
	if _, err := a.Book(context.Background(), bookingRequestFixture()); err != nil {
		t.Fatalf("Book: %v", err)
	}
	if gotBody["testIndicator"] != false {
		t.Fatalf("testIndicator = %v, want false once LiveBooking and CustomerNumber are both set", gotBody["testIndicator"])
	}
}

func TestBookSurfacesConsignmentErrors(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"consignments":[{"errors":[{"code":"BOOK-INPUT-014","messages":[{"message":"Invalid postal code"}]}]}]}`))
	}))
	defer srv.Close()
	a := New(Config{APIUID: "u", APIKey: "k", CustomerNumber: "1", BookingBaseURL: srv.URL})
	_, err := a.Book(context.Background(), bookingRequestFixture())
	if err == nil {
		t.Fatal("expected consignment error to surface")
	}
}

func TestBookRequiresCustomsCrossBorder(t *testing.T) {
	a := New(Config{APIUID: "u", APIKey: "k", CustomerNumber: "1", BookingBaseURL: "http://unused"})
	req := bookingRequestFixture()
	req.To.Country = "DE"
	if _, err := a.Book(context.Background(), req); err == nil {
		t.Fatal("cross-border booking without customs must fail before any API call")
	}
}

func TestTrackMapsEventsChronologically(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Query().Get("q") != "370438100000000002" {
			t.Errorf("tracking query = %q", r.URL.Query().Get("q"))
		}
		_, _ = w.Write([]byte(`{"consignmentSet":[{"packageSet":[{"packageNumber":"370438100000000002","eventSet":[
			{"description":"Delivered","status":"DELIVERED","dateIso":"2026-07-06T10:00:00+02:00"},
			{"description":"Collected","status":"COLLECTED","dateIso":"2026-07-04T08:00:00+02:00"}
		]}]}]}`))
	}))
	defer srv.Close()
	a := New(Config{APIUID: "u", APIKey: "k", CustomerNumber: "1", TrackingBaseURL: srv.URL})
	status, err := a.Track(context.Background(), "370438100000000002")
	if err != nil {
		t.Fatalf("Track: %v", err)
	}
	if len(status.Events) != 2 {
		t.Fatalf("events = %d", len(status.Events))
	}
	if status.Events[0].Status != "COLLECTED" || status.Events[1].Status != "DELIVERED" {
		t.Fatalf("events not chronological: %+v", status.Events)
	}
	if status.CurrentStatus != "DELIVERED" || status.ActualDelivery == nil {
		t.Fatalf("delivered mapping wrong: %+v", status)
	}
}
