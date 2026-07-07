package mock

import (
	"context"
	"errors"
	"testing"
	"time"

	"shipping-core/internal/carrier"
)

func baseRequest(segment carrier.Segment, weightKg float64) carrier.QuoteRequest {
	return carrier.QuoteRequest{
		From:    carrier.Address{PostalCode: "0150", City: "Oslo", Country: "NO"},
		To:      carrier.Address{PostalCode: "7010", City: "Trondheim", Country: "NO"},
		Package: carrier.Package{WeightKg: weightKg, LengthCm: 30, WidthCm: 20, HeightCm: 15},
		Segment: segment,
	}
}

func TestAdapter_Quote_SegmentFiltering(t *testing.T) {
	tests := []struct {
		name           string
		adapterSegment carrier.Segment
		requestSegment carrier.Segment
		wantQuote      bool
	}{
		{"both-adapter serves b2b", carrier.SegmentBoth, carrier.SegmentB2B, true},
		{"both-adapter serves b2c", carrier.SegmentBoth, carrier.SegmentB2C, true},
		{"b2b-only adapter serves b2b", carrier.SegmentB2B, carrier.SegmentB2B, true},
		{"b2b-only adapter rejects b2c", carrier.SegmentB2B, carrier.SegmentB2C, false},
		{"b2c-only adapter rejects b2b", carrier.SegmentB2C, carrier.SegmentB2B, false},
		{"b2c-only adapter serves b2c", carrier.SegmentB2C, carrier.SegmentB2C, true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			a := &Adapter{info: carrier.Info{Code: "test", Name: "Test", Segment: tt.adapterSegment}}
			quotes, err := a.Quote(context.Background(), baseRequest(tt.requestSegment, 5))
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if got := len(quotes) > 0; got != tt.wantQuote {
				t.Errorf("got quote=%v, want quote=%v (quotes=%v)", got, tt.wantQuote, quotes)
			}
		})
	}
}

func TestAdapter_Quote_ScalesWithWeight(t *testing.T) {
	a := &Adapter{
		info:            carrier.Info{Code: "test", Name: "Test", Segment: carrier.SegmentBoth},
		basePriceCents:  1000,
		pricePerKgCents: 100,
		transitDays:     2,
	}

	light, err := a.Quote(context.Background(), baseRequest(carrier.SegmentB2B, 1))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	heavy, err := a.Quote(context.Background(), baseRequest(carrier.SegmentB2B, 10))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if len(light) != 1 || len(heavy) != 1 {
		t.Fatalf("expected exactly one quote each, got light=%d heavy=%d", len(light), len(heavy))
	}
	if heavy[0].Price.AmountCents <= light[0].Price.AmountCents {
		t.Errorf("expected heavier package to cost more: light=%d heavy=%d",
			light[0].Price.AmountCents, heavy[0].Price.AmountCents)
	}
}

func TestAdapter_Quote_InjectedErr(t *testing.T) {
	wantErr := errors.New("upstream unavailable")
	a := &Adapter{
		info:        carrier.Info{Code: "test", Name: "Test", Segment: carrier.SegmentBoth},
		InjectedErr: wantErr,
	}

	_, err := a.Quote(context.Background(), baseRequest(carrier.SegmentB2B, 5))
	if !errors.Is(err, wantErr) {
		t.Errorf("got err=%v, want %v", err, wantErr)
	}
}

func TestAdapter_Quote_RespectsContextCancellation(t *testing.T) {
	a := &Adapter{
		info:            carrier.Info{Code: "test", Name: "Test", Segment: carrier.SegmentBoth},
		InjectedLatency: 200 * time.Millisecond,
	}

	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()

	_, err := a.Quote(ctx, baseRequest(carrier.SegmentB2B, 5))
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Errorf("got err=%v, want context.DeadlineExceeded", err)
	}
}

func TestDefaultCarriers_AllImplementContractWithoutError(t *testing.T) {
	for _, a := range DefaultCarriers() {
		t.Run(a.Info().Code, func(t *testing.T) {
			if a.Info().Code == "" {
				t.Error("Info().Code must not be empty")
			}
			// Every default carrier must return at least one quote for
			// its own declared segment (or "both").
			seg := a.Info().Segment
			if seg == carrier.SegmentBoth {
				seg = carrier.SegmentB2B
			}
			quotes, err := a.Quote(context.Background(), baseRequest(seg, 3))
			if err != nil {
				t.Fatalf("Quote returned error: %v", err)
			}
			if len(quotes) == 0 {
				t.Errorf("expected at least one quote for segment %s", seg)
			}

			// The booking lifecycle is implemented on the mock fleet now — see
			// booking_test.go for the full contract. Here only the invariants:
			// Book succeeds domestically, Label yields a PDF, and Track rejects
			// garbage tracking numbers rather than fabricating a history.
			if _, err := a.Book(context.Background(), carrier.BookingRequest{
				From: carrier.Address{Country: "NO", PostalCode: "0150"},
				To:   carrier.Address{Country: "NO", PostalCode: "7010"},
			}); err != nil {
				t.Errorf("Book returned error: %v", err)
			}
			if label, err := a.Label(context.Background(), "ref"); err != nil || len(label.Data) == 0 {
				t.Errorf("Label = %d bytes, err %v", len(label.Data), err)
			}
			if _, err := a.Track(context.Background(), "not-a-demo-number"); err == nil {
				t.Error("Track must reject unrecognized tracking numbers")
			}
		})
	}
}
