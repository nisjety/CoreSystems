// Package booking owns the shipment execution lifecycle: the two-step
// server-side confirmation gate, the carrier Book call, labels, pickups,
// tracking persistence, end-of-day manifests, and the audit log that ships
// WITH the feature (docs/ARCHITECTURE.md: the gate lives in the booking
// module, not in any UI or agent prompt — no client can bypass it).
package booking

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"time"

	"shipping-core/internal/carrier"
	"shipping-core/internal/docgen"
)

// ErrValidation marks caller mistakes (missing customs, unknown carrier…)
// that map to HTTP 400.
var ErrValidation = errors.New("validation")

// DeliveryObserver is notified the first time a booking is observed
// delivered. The hook point for cross-plane side effects (audit events,
// Data Plane evidence) that must not couple the booking package itself to
// their clients — see main.go wiring.
type DeliveryObserver interface {
	OnDelivered(ctx context.Context, rec Record, deliveredAt time.Time)
}

// Service coordinates the booking lifecycle across the store and the
// carrier adapter fleet.
type Service struct {
	store    *Store
	adapters map[string]carrier.Adapter
	logger   *slog.Logger
	observer DeliveryObserver // optional; nil-checked before use
}

func NewService(store *Store, adapters []carrier.Adapter, logger *slog.Logger) *Service {
	byCode := make(map[string]carrier.Adapter, len(adapters))
	for _, a := range adapters {
		byCode[a.Info().Code] = a
	}
	return &Service{store: store, adapters: byCode, logger: logger}
}

// SetDeliveryObserver wires an optional delivery-observed hook. Nil-safe:
// callers that don't need cross-plane side effects (e.g. tests) simply
// never call this.
func (s *Service) SetDeliveryObserver(observer DeliveryObserver) {
	s.observer = observer
}

// persistTrackingStatus upserts tracking events and, on the first
// observation of delivery, marks the booking delivered and notifies the
// observer. Shared by Tracking (read-triggered refresh) and
// RefreshOpenTracking (periodic batch refresh) so both paths feed F8
// identically.
func (s *Service) persistTrackingStatus(ctx context.Context, id string, rec Record, status carrier.TrackingStatus) {
	if err := s.store.UpsertTrackingEvents(ctx, rec.OrgID, id, status.Events); err != nil {
		s.logger.Warn("persist tracking events", "booking", id, "err", err)
	}
	if status.ActualDelivery == nil || rec.ActualDeliveredAt != nil {
		return
	}
	if err := s.store.MarkDelivered(ctx, rec.OrgID, id, *status.ActualDelivery); err != nil {
		s.logger.Warn("mark delivered", "booking", id, "err", err)
		return
	}
	s.store.Audit(ctx, rec.OrgID, id, "delivered", "carrier-tracking", map[string]any{
		"actual_delivered_at": status.ActualDelivery.Format(time.RFC3339),
	})
	if s.observer != nil {
		rec.ActualDeliveredAt = status.ActualDelivery
		s.observer.OnDelivered(ctx, rec, *status.ActualDelivery)
	}
}

// CreateInput is the normalized request to open a booking (step 1 of 2).
type CreateInput struct {
	OrgID              string
	ActorPrincipalType string
	QuoteRef           string
	CarrierCode        string
	CarrierName        string
	ServiceName        string
	Price              carrier.Money
	From               carrier.Address
	To                 carrier.Address
	Package            carrier.Package
	Customs            *carrier.CustomsInfo
	BookedBy           string
	ApprovalID         string
	IdempotencyKey     string
	ZDR                bool
	RetentionUntil     *time.Time
	EstimatedDelivery  *time.Time // optional — see carrier.BookingRequest doc
}

// CreateResult returns the booking id plus the single-use confirmation
// token. The token is shown ONCE; confirming requires presenting it back.
type CreateResult struct {
	BookingID         string `json:"booking_id"`
	Status            string `json:"status"`
	ConfirmationToken string `json:"confirmation_token"`
	RequiresCustoms   bool   `json:"requires_customs"`
}

// Create validates and persists a pending booking. Nothing is sent to any
// carrier here — that only happens in Confirm, after the gate.
func (s *Service) Create(ctx context.Context, in CreateInput) (CreateResult, error) {
	if in.OrgID == "" || in.BookedBy == "" || in.ActorPrincipalType == "" {
		return CreateResult{}, fmt.Errorf("%w: verified organization and actor are required", ErrValidation)
	}
	adapter, ok := s.adapters[in.CarrierCode]
	if !ok {
		return CreateResult{}, fmt.Errorf("%w: unknown carrier %q", ErrValidation, in.CarrierCode)
	}
	crossBorder := in.From.Country != in.To.Country
	if crossBorder && (in.Customs == nil || len(in.Customs.Items) == 0) {
		return CreateResult{}, fmt.Errorf("%w: cross-border shipment (%s→%s) requires a customs declaration with at least one item", ErrValidation, in.From.Country, in.To.Country)
	}
	if in.CarrierName == "" {
		in.CarrierName = adapter.Info().Name
	}

	token, err := newToken()
	if err != nil {
		return CreateResult{}, err
	}
	requestDigest, err := bookingRequestDigest(in)
	if err != nil {
		return CreateResult{}, err
	}
	id, storedToken, status, err := s.store.CreateBooking(ctx, Record{
		OrgID:              in.OrgID,
		ActorPrincipalType: in.ActorPrincipalType,
		ConfirmationToken:  token,
		QuoteRef:           in.QuoteRef,
		CarrierCode:        in.CarrierCode,
		CarrierName:        in.CarrierName,
		ServiceName:        in.ServiceName,
		Price:              in.Price,
		From:               in.From,
		To:                 in.To,
		Package:            in.Package,
		Customs:            in.Customs,
		BookedBy:           in.BookedBy,
		ApprovalID:         in.ApprovalID,
		IdempotencyKey:     in.IdempotencyKey,
		RequestDigest:      requestDigest,
		ZDR:                in.ZDR,
		RetentionUntil:     in.RetentionUntil,
		EstimatedDelivery:  in.EstimatedDelivery,
	})
	if err != nil {
		return CreateResult{}, err
	}
	if storedToken == token {
		s.store.Audit(ctx, in.OrgID, id, "created", in.BookedBy, map[string]any{
			"carrier": in.CarrierCode, "service": in.ServiceName,
			"price_cents": in.Price.AmountCents, "cross_border": crossBorder,
		})
	}
	return CreateResult{
		BookingID:         id,
		Status:            status,
		ConfirmationToken: storedToken,
		RequiresCustoms:   crossBorder,
	}, nil
}

// Confirm is step 2: the gate. The token must match AND the booking must
// still be pending (atomic claim in the store); only then is the carrier
// called. Every outcome is audited.
func (s *Service) Confirm(ctx context.Context, orgID, id, token, actor string) (Record, error) {
	if err := s.store.ClaimForConfirmation(ctx, orgID, id, token, actor); err != nil {
		if errors.Is(err, ErrGate) {
			s.store.Audit(ctx, orgID, id, "confirm_rejected", actor, map[string]any{"reason": "token mismatch, expired, wrong tenant, or not pending"})
		}
		return Record{}, err
	}
	rec, err := s.store.GetBooking(ctx, orgID, id)
	if err != nil {
		return Record{}, err
	}
	adapter, ok := s.adapters[rec.CarrierCode]
	if !ok {
		_ = s.store.MarkFailed(ctx, orgID, id, "carrier adapter no longer registered")
		return Record{}, fmt.Errorf("%w: carrier %q no longer registered", ErrValidation, rec.CarrierCode)
	}

	booked, err := adapter.Book(ctx, carrier.BookingRequest{
		QuoteRef:          rec.QuoteRef,
		ServiceName:       rec.ServiceName,
		Price:             rec.Price,
		From:              rec.From,
		To:                rec.To,
		Package:           rec.Package,
		Customs:           rec.Customs,
		BookedBy:          rec.BookedBy,
		EstimatedDelivery: rec.EstimatedDelivery,
	})
	if err != nil {
		_ = s.store.MarkFailed(ctx, orgID, id, err.Error())
		s.store.Audit(ctx, orgID, id, "book_failed", actor, map[string]any{"error": err.Error()})
		return Record{}, fmt.Errorf("carrier booking failed: %w", err)
	}

	// Label + ZPL: best-effort at booking time so downloads never depend on
	// the carrier being up later. A label failure does not undo the booking.
	var label carrier.Label
	if l, lerr := adapter.Label(ctx, booked.BookingRef); lerr == nil {
		label = l
	} else {
		s.logger.Warn("label fetch failed after booking", "booking", id, "err", lerr)
		s.store.Audit(ctx, orgID, id, "label_failed", actor, map[string]any{"error": lerr.Error()})
	}
	var zpl string
	if z, ok := adapter.(interface{ ZPL(string) string }); ok {
		zpl = z.ZPL(booked.BookingRef)
	}

	// Cross-border: render the CN22-style customs document alongside.
	var customsDoc []byte
	if rec.Customs != nil {
		customsDoc = customsDocument(rec, booked)
	}

	if err := s.store.MarkBooked(ctx, orgID, id, booked, label, zpl, customsDoc); err != nil {
		return Record{}, err
	}
	s.store.Audit(ctx, orgID, id, "booked", actor, map[string]any{
		"booking_ref": booked.BookingRef, "tracking_no": booked.TrackingNo,
	})
	return s.store.GetBooking(ctx, orgID, id)
}

// Cancel cancels a booking (carrier-side when supported) and audits it.
func (s *Service) Cancel(ctx context.Context, orgID, id, actor string) error {
	rec, err := s.store.GetBooking(ctx, orgID, id)
	if err != nil {
		return err
	}
	if adapter, ok := s.adapters[rec.CarrierCode]; ok && rec.BookingRef != "" {
		if canceller, ok := adapter.(carrier.BookingCanceller); ok {
			if err := canceller.CancelBooking(ctx, rec.BookingRef); err != nil {
				s.store.Audit(ctx, orgID, id, "cancel_failed", actor, map[string]any{"error": err.Error()})
				return fmt.Errorf("carrier cancel failed: %w", err)
			}
		}
	}
	if err := s.store.MarkCancelled(ctx, orgID, id); err != nil {
		return err
	}
	s.store.Audit(ctx, orgID, id, "cancelled", actor, nil)
	return nil
}

// SchedulePickup orders carrier collection for a booked shipment.
func (s *Service) SchedulePickup(ctx context.Context, orgID, id, actor string, req carrier.PickupRequest) (carrier.Pickup, error) {
	rec, err := s.store.GetBooking(ctx, orgID, id)
	if err != nil {
		return carrier.Pickup{}, err
	}
	if rec.Status != "booked" {
		return carrier.Pickup{}, fmt.Errorf("%w: pickup requires a booked shipment (status %s)", ErrValidation, rec.Status)
	}
	adapter := s.adapters[rec.CarrierCode]
	scheduler, ok := adapter.(carrier.PickupScheduler)
	if !ok {
		return carrier.Pickup{}, fmt.Errorf("%w: %s does not support pickup ordering through this integration", ErrValidation, rec.CarrierName)
	}
	if len(req.BookingRefs) == 0 {
		req.BookingRefs = []string{rec.BookingRef}
	}
	if req.Address.Name == "" {
		req.Address = rec.From
	}
	pickup, err := scheduler.SchedulePickup(ctx, req)
	if err != nil {
		s.store.Audit(ctx, orgID, id, "pickup_failed", actor, map[string]any{"error": err.Error()})
		return carrier.Pickup{}, fmt.Errorf("carrier pickup failed: %w", err)
	}
	if err := s.store.SetPickup(ctx, orgID, id, pickup); err != nil {
		return carrier.Pickup{}, err
	}
	s.store.Audit(ctx, orgID, id, "pickup_scheduled", actor, map[string]any{
		"pickup_ref": pickup.PickupRef, "date": pickup.Date.Format("2006-01-02"),
	})
	return pickup, nil
}

// Tracking refreshes (on read) and returns a booking's tracking history.
type Tracking struct {
	BookingID     string                  `json:"booking_id"`
	TrackingNo    string                  `json:"tracking_no"`
	CurrentStatus string                  `json:"current_status"`
	Events        []carrier.TrackingEvent `json:"events"`
}

// Tracking polls the carrier for fresh events, persists them idempotently
// along with any observed delivery (the F8 reliability signal), and
// returns the combined history. A carrier outage degrades to the
// persisted events.
func (s *Service) Tracking(ctx context.Context, orgID, id string) (Tracking, error) {
	rec, err := s.store.GetBooking(ctx, orgID, id)
	if err != nil {
		return Tracking{}, err
	}
	if rec.TrackingNo == "" {
		return Tracking{}, fmt.Errorf("%w: booking has no tracking number yet (status %s)", ErrValidation, rec.Status)
	}
	current := ""
	if adapter, ok := s.adapters[rec.CarrierCode]; ok {
		if status, terr := adapter.Track(ctx, rec.TrackingNo); terr == nil {
			current = status.CurrentStatus
			s.persistTrackingStatus(ctx, id, rec, status)
		} else {
			s.logger.Warn("carrier tracking refresh failed; serving persisted events", "booking", id, "err", terr)
		}
	}
	events, err := s.store.ListTrackingEvents(ctx, orgID, id)
	if err != nil {
		return Tracking{}, err
	}
	if current == "" && len(events) > 0 {
		current = events[len(events)-1].Status
	}
	return Tracking{BookingID: id, TrackingNo: rec.TrackingNo, CurrentStatus: current, Events: events}, nil
}

// RefreshOpenTracking polls every open (booked, undelivered) shipment
// within window and persists whatever each carrier reports — the F8 data
// substrate would otherwise depend entirely on someone opening each
// booking's tracking page. Per-booking carrier failures are logged and
// skipped; one carrier outage never blocks the rest of the batch.
func (s *Service) RefreshOpenTracking(ctx context.Context, window time.Duration) (refreshed int, err error) {
	candidates, err := s.store.ListOpenTrackable(ctx, window)
	if err != nil {
		return 0, fmt.Errorf("list open trackable: %w", err)
	}
	for _, c := range candidates {
		adapter, ok := s.adapters[c.CarrierCode]
		if !ok {
			continue
		}
		status, terr := adapter.Track(ctx, c.TrackingNo)
		if terr != nil {
			s.logger.Warn("refresh open tracking: carrier track failed", "booking", c.ID, "carrier", c.CarrierCode, "err", terr)
			continue
		}
		rec, gerr := s.store.GetBooking(ctx, c.OrgID, c.ID)
		if gerr != nil {
			s.logger.Warn("refresh open tracking: reload booking failed", "booking", c.ID, "err", gerr)
			continue
		}
		s.persistTrackingStatus(ctx, c.ID, rec, status)
		refreshed++
	}
	return refreshed, nil
}

// BuildManifest groups today's unmanifested booked shipments for a carrier
// into an end-of-day manifest with a rendered summary document.
func (s *Service) BuildManifest(ctx context.Context, orgID, carrierCode, actor string) (string, int, error) {
	if _, ok := s.adapters[carrierCode]; !ok {
		return "", 0, fmt.Errorf("%w: unknown carrier %q", ErrValidation, carrierCode)
	}
	id, included, err := s.store.CreateManifest(ctx, orgID, carrierCode, actor, nil)
	if err != nil {
		return "", 0, err
	}
	doc := manifestDocument(carrierCode, included)
	if err := s.store.SetManifestDocument(ctx, orgID, id, doc); err != nil {
		return "", 0, err
	}
	for _, rec := range included {
		s.store.Audit(ctx, orgID, rec.ID, "manifested", actor, map[string]any{"manifest_id": id})
	}
	return id, len(included), nil
}

func bookingRequestDigest(in CreateInput) (string, error) {
	payload, err := json.Marshal(in)
	if err != nil {
		return "", fmt.Errorf("encode booking request digest: %w", err)
	}
	digest := sha256.Sum256(payload)
	return hex.EncodeToString(digest[:]), nil
}

func newToken() (string, error) {
	buf := make([]byte, 24)
	if _, err := rand.Read(buf); err != nil {
		return "", fmt.Errorf("generate confirmation token: %w", err)
	}
	return hex.EncodeToString(buf), nil
}

// customsDocument renders a CN22-style declaration for a booked shipment.
// It is the shipper's copy of the declared data — carriers receive the same
// data structurally through their booking APIs.
func customsDocument(rec Record, booked carrier.Booking) []byte {
	lines := []docgen.Line{
		docgen.H1("CUSTOMS DECLARATION (CN22)"),
		docgen.Txt(fmt.Sprintf("Booking: %s    Tracking: %s", booked.BookingRef, booked.TrackingNo)),
		docgen.Txt(fmt.Sprintf("From: %s, %s %s, %s", rec.From.Name, rec.From.PostalCode, rec.From.City, rec.From.Country)),
		docgen.Txt(fmt.Sprintf("To:   %s, %s %s, %s", rec.To.Name, rec.To.PostalCode, rec.To.City, rec.To.Country)),
		docgen.Txt(""),
		docgen.H2("Contents (" + rec.Customs.ContentsType + ")"),
	}
	for _, item := range rec.Customs.Items {
		lines = append(lines, docgen.Txt(fmt.Sprintf(
			"%dx %s — %d.%02d %s, %.2f kg, HS %s, origin %s",
			item.Quantity, item.Description,
			item.ValueCents/100, item.ValueCents%100, item.Currency,
			item.WeightKg, item.HSCode, item.OriginCountry,
		)))
	}
	total := rec.Customs.TotalValueCents()
	lines = append(lines,
		docgen.Txt(""),
		docgen.H2(fmt.Sprintf("Total declared value: %d.%02d %s", total/100, total%100, rec.Price.Currency)),
	)
	if rec.Customs.Incoterms != "" {
		lines = append(lines, docgen.Txt("Incoterms: "+rec.Customs.Incoterms))
	}
	lines = append(lines, docgen.Txt("Generated by Velion shipping-core "+time.Now().UTC().Format(time.RFC3339)))
	return docgen.PDF(docgen.PageA4, lines)
}

// manifestDocument renders the end-of-day manifest summary.
func manifestDocument(carrierCode string, included []Record) []byte {
	lines := []docgen.Line{
		docgen.H1("SHIPMENT MANIFEST — " + carrierCode),
		docgen.Txt("Date: " + time.Now().Format("2006-01-02")),
		docgen.Txt(fmt.Sprintf("Shipments: %d", len(included))),
		docgen.Txt(""),
	}
	for _, rec := range included {
		lines = append(lines, docgen.Txt(fmt.Sprintf("%s  %s  (%s)", rec.BookingRef, rec.TrackingNo, rec.ServiceName)))
	}
	lines = append(lines, docgen.Txt(""), docgen.Txt("Generated by Velion shipping-core."))
	return docgen.PDF(docgen.PageA4, lines)
}
