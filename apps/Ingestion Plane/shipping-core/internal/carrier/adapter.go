// Package carrier defines the CarrierAdapter contract that every shipping
// carrier integration implements, and the domain types shared across them.
// This is the seam the rest of the system is built around: quoteengine,
// booking, and the future agent-service all talk to carriers exclusively
// through this interface, never to a specific carrier's API shape directly.
package carrier

import (
	"context"
	"time"
)

// Segment identifies whether a carrier service targets business or
// consumer recipients. Some carriers (e.g. Bring) offer both; most B2C
// home-delivery carriers (Helthjem, Porterbuddy) only ever appear here.
type Segment string

const (
	SegmentB2B  Segment = "b2b"
	SegmentB2C  Segment = "b2c"
	SegmentBoth Segment = "both"
)

// Mode states which upstream environment produced a carrier capability.
type Mode string

const (
	ModeProduction Mode = "production"
	ModeSandbox    Mode = "sandbox"
	ModeMock       Mode = "mock"
)

// Info describes a carrier integration itself, independent of any single
// quote request.
type Info struct {
	Code           string // stable identifier, e.g. "bring", "postnord", "mock-dhl"
	Name           string // display name, e.g. "Bring"
	Segment        Segment
	Mode           Mode
	VerifiedAt     *time.Time
	DegradedReason string
}

// Address is the minimal shape every carrier adapter needs to compute a
// quote or create a booking. Adapters translate this into whatever shape
// their upstream API expects.
//
// Phone/Email are booking-only (a quote never needs them): some carriers
// (Bring included) reject a booking outright unless the recipient has at
// least one, since it's what they use to send delivery notifications.
// Both are optional here — an adapter that has neither simply omits the
// carrier's notification block rather than failing itself.
type Address struct {
	Name       string `json:"name"`
	Street     string `json:"street"`
	PostalCode string `json:"postal_code"`
	City       string `json:"city"`
	Country    string `json:"country"` // ISO 3166-1 alpha-2, e.g. "NO"
	IsBusiness bool   `json:"is_business"`
	Phone      string `json:"phone,omitempty"`
	Email      string `json:"email,omitempty"`
}

// Package describes the physical properties of what's being shipped.
type Package struct {
	WeightKg      float64 `json:"weight_kg"`
	LengthCm      float64 `json:"length_cm"`
	WidthCm       float64 `json:"width_cm"`
	HeightCm      float64 `json:"height_cm"`
	DangerousGood bool    `json:"dangerous_good"`
}

// QuoteRequest is the normalized input to every adapter's Quote call. It is
// carrier-agnostic; adapters are responsible for rejecting requests they
// cannot serve (e.g. a B2C-only adapter given a business recipient) by
// returning an empty slice rather than an error.
type QuoteRequest struct {
	From            Address
	To              Address
	Package         Package
	Segment         Segment
	DesiredDelivery *time.Time // optional: "must arrive by" constraint
}

// Money avoids floating-point rounding surprises in prices.
type Money struct {
	AmountCents int64  `json:"amount_cents"`
	Currency    string `json:"currency"` // ISO 4217, e.g. "NOK"
}

// Quote is one priced shipping option returned by a single carrier for a
// single QuoteRequest. ReliabilityScore is nil until the reliability
// module exists (a later phase) — callers must treat nil as "unknown", not
// "zero".
type Quote struct {
	// Provenance is supplied by quoteengine from the configured adapter, never
	// from model arguments. A quote currently covers exactly one package.
	Environment       string    `json:"environment"`
	IsMock            bool      `json:"is_mock"`
	QuotedAt          time.Time `json:"quoted_at"`
	PackageCount      int       `json:"package_count"`
	CarrierCode       string    `json:"carrier_code"`
	CarrierName       string    `json:"carrier_name"`
	ServiceName       string    `json:"service_name"`
	Price             Money     `json:"price"`
	EstimatedDelivery time.Time `json:"estimated_delivery"`
	TransitDays       int       `json:"transit_days"`
	Features          []string  `json:"features"`          // e.g. "tracking", "home_delivery", "pickup_point"
	ReliabilityScore  *float64  `json:"reliability_score"` // 0.0-1.0 share of on-time deliveries; nil = unknown
}

// CustomsItem is one contents line on a customs declaration (CN22/CN23-style
// data). HSCode is the harmonized system tariff code; OriginCountry is ISO
// 3166-1 alpha-2.
type CustomsItem struct {
	Description   string  `json:"description"`
	Quantity      int     `json:"quantity"`
	ValueCents    int64   `json:"value_cents"`
	Currency      string  `json:"currency"`
	WeightKg      float64 `json:"weight_kg"`
	HSCode        string  `json:"hs_code"`
	OriginCountry string  `json:"origin_country"`
}

// CustomsInfo is the export declaration attached to a cross-border booking.
// Required whenever From.Country != To.Country (Norway is outside the EU, so
// effectively every non-domestic shipment). ContentsType follows the CN22
// vocabulary: "merchandise", "gift", "documents", "sample", "return".
type CustomsInfo struct {
	ContentsType string        `json:"contents_type"`
	Items        []CustomsItem `json:"items"`
	Incoterms    string        `json:"incoterms,omitempty"` // e.g. "DAP", "DDP"
	InvoiceNo    string        `json:"invoice_no,omitempty"`
}

// TotalValueCents sums the declared item values (quantity-weighted).
func (c CustomsInfo) TotalValueCents() int64 {
	var total int64
	for _, item := range c.Items {
		qty := int64(item.Quantity)
		if qty < 1 {
			qty = 1
		}
		total += item.ValueCents * qty
	}
	return total
}

// BookingRequest confirms a specific Quote for execution. QuoteRef ties a
// booking back to the exact priced option the user picked; ServiceName and
// Price snapshot that option so the adapter books the same product the user
// saw (quotes are not persisted carrier-side).
type BookingRequest struct {
	QuoteRef      string       `json:"quote_ref"`
	ServiceName   string       `json:"service_name"`
	Price         Money        `json:"price"`
	From          Address      `json:"from"`
	To            Address      `json:"to"`
	Package       Package      `json:"package"`
	Customs       *CustomsInfo `json:"customs,omitempty"` // required cross-border
	BookedBy      string       `json:"booked_by"`         // user identifier, for the audit log
	VismaOrderRef *string      `json:"visma_order_ref"`   // ERP seam (own ERP later)
	// EstimatedDelivery is the quote's promised delivery date, snapshotted
	// at booking time (not sent to any carrier — carriers compute their own
	// ETA). Optional: callers that book without going through a quote first
	// (or older callers not yet updated) simply leave this nil, and the
	// booking never enters the F8 reliability-scoring population — no
	// fabricated estimate is substituted.
	EstimatedDelivery *time.Time `json:"estimated_delivery,omitempty"`
}

// Booking is the result of successfully placing a shipment with a carrier.
type Booking struct {
	CarrierCode string    `json:"carrier_code"`
	BookingRef  string    `json:"booking_ref"` // carrier's own order/booking identifier
	TrackingNo  string    `json:"tracking_no"`
	Price       Money     `json:"price"`
	CreatedAt   time.Time `json:"created_at"`
}

// Label is a downloadable shipping label, typically PDF or ZPL.
type Label struct {
	ContentType string `json:"content_type"` // e.g. "application/pdf"
	Data        []byte `json:"data"`
}

// TrackingEvent is a single status update in a shipment's history.
type TrackingEvent struct {
	Status      string    `json:"status"`
	Description string    `json:"description"`
	OccurredAt  time.Time `json:"occurred_at"`
}

// TrackingStatus is the current state of a shipment plus its history.
type TrackingStatus struct {
	TrackingNo        string          `json:"tracking_no"`
	CurrentStatus     string          `json:"current_status"`
	EstimatedDelivery *time.Time      `json:"estimated_delivery"`
	ActualDelivery    *time.Time      `json:"actual_delivery"`
	Events            []TrackingEvent `json:"events"`
}

// PickupRequest orders carrier collection of one or more booked shipments
// from an address on a given date/time window.
type PickupRequest struct {
	BookingRefs []string  `json:"booking_refs"`
	Address     Address   `json:"address"`
	Date        time.Time `json:"date"`
	TimeFrom    string    `json:"time_from"` // "HH:MM", local time
	TimeTo      string    `json:"time_to"`
	Note        string    `json:"note,omitempty"` // e.g. "ring doorbell at ramp B"
}

// Pickup is the carrier's confirmation of a scheduled collection.
type Pickup struct {
	CarrierCode string    `json:"carrier_code"`
	PickupRef   string    `json:"pickup_ref"` // carrier's confirmation number
	Date        time.Time `json:"date"`
	TimeFrom    string    `json:"time_from"`
	TimeTo      string    `json:"time_to"`
	ConfirmedAt time.Time `json:"confirmed_at"`
}

// Adapter is the contract every carrier integration implements. All
// methods must respect ctx cancellation/deadline — quoteengine relies on
// this to enforce a per-carrier timeout during fan-out without leaking
// goroutines.
type Adapter interface {
	// Info returns static metadata about this carrier integration.
	Info() Info

	// Quote returns priced options for req. An adapter that cannot serve
	// req (wrong segment, unsupported destination, etc.) returns (nil, nil)
	// rather than an error — an error return means the lookup itself
	// failed (timeout, upstream 5xx, auth failure), not "no options".
	Quote(ctx context.Context, req QuoteRequest) ([]Quote, error)

	// Book places the shipment with the carrier for a previously-returned
	// quote. Never called without explicit user confirmation upstream —
	// internal/booking enforces that gate server-side.
	Book(ctx context.Context, req BookingRequest) (Booking, error)

	// Label retrieves the shipping label for an existing booking.
	Label(ctx context.Context, bookingRef string) (Label, error)

	// Track retrieves current status for a tracking number.
	Track(ctx context.Context, trackingNo string) (TrackingStatus, error)
}

// PickupScheduler is the optional pickup-ordering capability. Checked via
// type assertion (consumer-defined, like quoteengine.Quoter) so quote-only
// adapters stay valid without stubbing methods they cannot honour.
type PickupScheduler interface {
	SchedulePickup(ctx context.Context, req PickupRequest) (Pickup, error)
}

// BookingCanceller is the optional cancellation capability.
type BookingCanceller interface {
	CancelBooking(ctx context.Context, bookingRef string) error
}
