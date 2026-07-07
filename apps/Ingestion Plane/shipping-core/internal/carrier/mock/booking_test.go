package mock

import (
	"bytes"
	"context"
	"testing"
	"time"

	"shipping-core/internal/carrier"
)

func demoAdapter() *Adapter {
	adapters := DefaultCarriers()
	return adapters[0].(*Adapter)
}

func demoRequest() carrier.BookingRequest {
	return carrier.BookingRequest{
		QuoteRef:    "q1",
		ServiceName: "Demo Standard",
		Price:       carrier.Money{AmountCents: 13300, Currency: "NOK"},
		From:        carrier.Address{Name: "Velion AS", PostalCode: "0150", City: "Oslo", Country: "NO", IsBusiness: true},
		To:          carrier.Address{Name: "Kari", PostalCode: "7010", City: "Trondheim", Country: "NO"},
		Package:     carrier.Package{WeightKg: 8, LengthCm: 40, WidthCm: 30, HeightCm: 20},
		BookedBy:    "test",
	}
}

func TestMockBookProducesTrackableReferences(t *testing.T) {
	a := demoAdapter()
	booked, err := a.Book(context.Background(), demoRequest())
	if err != nil {
		t.Fatalf("Book: %v", err)
	}
	if booked.BookingRef == "" || booked.TrackingNo == "" {
		t.Fatalf("booking missing refs: %+v", booked)
	}

	status, err := a.Track(context.Background(), booked.TrackingNo)
	if err != nil {
		t.Fatalf("Track: %v", err)
	}
	if status.CurrentStatus != "booked" {
		t.Fatalf("fresh booking status = %q, want booked", status.CurrentStatus)
	}
	if len(status.Events) != 1 {
		t.Fatalf("fresh booking events = %d, want 1", len(status.Events))
	}
	if status.EstimatedDelivery == nil {
		t.Fatalf("undelivered shipment must carry an ETA")
	}
}

func TestMockBookRequiresCustomsCrossBorder(t *testing.T) {
	a := demoAdapter()
	req := demoRequest()
	req.To.Country = "SE"
	if _, err := a.Book(context.Background(), req); err == nil {
		t.Fatal("cross-border booking without customs must fail")
	}
	req.Customs = &carrier.CustomsInfo{
		ContentsType: "merchandise",
		Items:        []carrier.CustomsItem{{Description: "widget", Quantity: 1, ValueCents: 10000, Currency: "NOK"}},
	}
	if _, err := a.Book(context.Background(), req); err != nil {
		t.Fatalf("cross-border booking with customs: %v", err)
	}
}

func TestMockLabelIsValidPDFWithZPL(t *testing.T) {
	a := demoAdapter()
	label, err := a.Label(context.Background(), "mock-bring-123")
	if err != nil {
		t.Fatalf("Label: %v", err)
	}
	if label.ContentType != "application/pdf" {
		t.Fatalf("label content type = %q", label.ContentType)
	}
	if !bytes.HasPrefix(label.Data, []byte("%PDF-1.4")) {
		t.Fatalf("label does not start with a PDF header")
	}
	if !bytes.Contains(label.Data, []byte("DEMO LABEL")) {
		t.Fatalf("demo label must be watermarked as demo")
	}
	zpl := a.ZPL("mock-bring-123")
	if len(zpl) == 0 || zpl[:3] != "^XA" {
		t.Fatalf("ZPL output malformed: %q", zpl)
	}
}

func TestMockTrackingProgressesOverTime(t *testing.T) {
	a := demoAdapter()
	// A tracking number "booked" 10 stages ago must be delivered.
	old := time.Now().Add(-10 * demoTransitScale).Unix()
	trackingNo := "VD" + itoa(old) + "0042"
	status, err := a.Track(context.Background(), trackingNo)
	if err != nil {
		t.Fatalf("Track: %v", err)
	}
	if status.CurrentStatus != "delivered" {
		t.Fatalf("old shipment status = %q, want delivered", status.CurrentStatus)
	}
	if status.ActualDelivery == nil {
		t.Fatalf("delivered shipment must carry ActualDelivery")
	}
	if len(status.Events) != len(mockJourney) {
		t.Fatalf("delivered shipment events = %d, want %d", len(status.Events), len(mockJourney))
	}
}

func TestMockSchedulePickup(t *testing.T) {
	a := demoAdapter()
	pickup, err := a.SchedulePickup(context.Background(), carrier.PickupRequest{
		Date: time.Now().AddDate(0, 0, 1), TimeFrom: "08:00", TimeTo: "16:00",
	})
	if err != nil {
		t.Fatalf("SchedulePickup: %v", err)
	}
	if pickup.PickupRef == "" {
		t.Fatalf("pickup missing confirmation ref")
	}
	if _, err := a.SchedulePickup(context.Background(), carrier.PickupRequest{}); err == nil {
		t.Fatal("pickup without a date must fail")
	}
}

func itoa(v int64) string {
	if v == 0 {
		return "0"
	}
	var digits []byte
	for v > 0 {
		digits = append([]byte{byte('0' + v%10)}, digits...)
		v /= 10
	}
	return string(digits)
}
