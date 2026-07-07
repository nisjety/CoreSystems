package booking

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"shipping-core/internal/carrier"
)

// ErrNotFound marks a booking/manifest id that does not exist.
var ErrNotFound = errors.New("not found")

// ErrGate marks a confirmation-gate violation: wrong token, or the booking
// is not awaiting confirmation (already confirmed, failed, or cancelled).
var ErrGate = errors.New("confirmation gate")

// Record is a booking row. Addresses/package/customs are snapshotted as
// jsonb so a booking is fully self-describing years later, independent of
// any later model changes.
type Record struct {
	ID                string               `json:"id"`
	Status            string               `json:"status"`
	QuoteRef          string               `json:"quote_ref"`
	CarrierCode       string               `json:"carrier_code"`
	CarrierName       string               `json:"carrier_name"`
	ServiceName       string               `json:"service_name"`
	Price             carrier.Money        `json:"price"`
	From              carrier.Address      `json:"from"`
	To                carrier.Address      `json:"to"`
	Package           carrier.Package      `json:"package"`
	Customs           *carrier.CustomsInfo `json:"customs,omitempty"`
	BookingRef        string               `json:"booking_ref,omitempty"`
	TrackingNo        string               `json:"tracking_no,omitempty"`
	BookedAt          *time.Time           `json:"booked_at,omitempty"`
	LabelContentType  string               `json:"-"`
	LabelData         []byte               `json:"-"`
	LabelZPL          string               `json:"-"`
	CustomsDocPDF     []byte               `json:"-"`
	HasLabel          bool                 `json:"has_label"`
	HasCustomsDoc     bool                 `json:"has_customs_doc"`
	PickupRef         string               `json:"pickup_ref,omitempty"`
	PickupDate        *time.Time           `json:"pickup_date,omitempty"`
	PickupWindow      string               `json:"pickup_window,omitempty"`
	ManifestID        string               `json:"manifest_id,omitempty"`
	BookedBy          string               `json:"booked_by"`
	ErrorMessage      string               `json:"error_message,omitempty"`
	ConfirmationToken string               `json:"-"` // never serialized on reads
	CreatedAt         time.Time            `json:"created_at"`
	UpdatedAt         time.Time            `json:"updated_at"`
}

// Store owns all booking-related SQL.
type Store struct {
	pool *pgxpool.Pool
}

func NewStore(pool *pgxpool.Pool) *Store { return &Store{pool: pool} }

// CreateBooking inserts a pending_confirmation row and returns its id. The
// confirmation token is stored server-side; it is returned to the creating
// client ONCE and never readable again.
func (s *Store) CreateBooking(ctx context.Context, rec Record) (string, error) {
	fromJSON, _ := json.Marshal(rec.From)
	toJSON, _ := json.Marshal(rec.To)
	pkgJSON, _ := json.Marshal(rec.Package)
	var customsJSON []byte
	if rec.Customs != nil {
		customsJSON, _ = json.Marshal(rec.Customs)
	}

	var id string
	err := s.pool.QueryRow(ctx, `
		INSERT INTO bookings (
			status, confirmation_token, quote_ref, carrier_code, carrier_name,
			service_name, price_amount_cents, price_currency,
			from_address, to_address, package, customs, booked_by
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
		RETURNING id`,
		"pending_confirmation", rec.ConfirmationToken, rec.QuoteRef, rec.CarrierCode,
		rec.CarrierName, rec.ServiceName, rec.Price.AmountCents, rec.Price.Currency,
		fromJSON, toJSON, pkgJSON, nullableJSON(customsJSON), rec.BookedBy,
	).Scan(&id)
	if err != nil {
		return "", fmt.Errorf("insert booking: %w", err)
	}
	return id, nil
}

func nullableJSON(b []byte) any {
	if len(b) == 0 {
		return nil
	}
	return b
}

// GetBooking loads one booking with its snapshots.
func (s *Store) GetBooking(ctx context.Context, id string) (Record, error) {
	var (
		rec         Record
		fromJSON    []byte
		toJSON      []byte
		pkgJSON     []byte
		customsJSON []byte
		labelCT     *string
		labelZPL    *string
		bookingRef  *string
		trackingNo  *string
		pickupRef   *string
		pickupWin   *string
		manifestID  *string
		errMsg      *string
		confirmTok  string
	)
	err := s.pool.QueryRow(ctx, `
		SELECT id, status, confirmation_token, quote_ref, carrier_code, carrier_name,
		       service_name, price_amount_cents, price_currency,
		       from_address, to_address, package, customs,
		       booking_ref, tracking_no, booked_at,
		       label_content_type, label_data, label_zpl, customs_doc_pdf,
		       pickup_ref, pickup_date, pickup_window, manifest_id,
		       booked_by, error_message, created_at, updated_at
		FROM bookings WHERE id = $1`, id).Scan(
		&rec.ID, &rec.Status, &confirmTok, &rec.QuoteRef, &rec.CarrierCode, &rec.CarrierName,
		&rec.ServiceName, &rec.Price.AmountCents, &rec.Price.Currency,
		&fromJSON, &toJSON, &pkgJSON, &customsJSON,
		&bookingRef, &trackingNo, &rec.BookedAt,
		&labelCT, &rec.LabelData, &labelZPL, &rec.CustomsDocPDF,
		&pickupRef, &rec.PickupDate, &pickupWin, &manifestID,
		&rec.BookedBy, &errMsg, &rec.CreatedAt, &rec.UpdatedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return Record{}, ErrNotFound
	}
	if err != nil {
		return Record{}, fmt.Errorf("select booking: %w", err)
	}
	rec.ConfirmationToken = confirmTok
	_ = json.Unmarshal(fromJSON, &rec.From)
	_ = json.Unmarshal(toJSON, &rec.To)
	_ = json.Unmarshal(pkgJSON, &rec.Package)
	if len(customsJSON) > 0 {
		var c carrier.CustomsInfo
		if json.Unmarshal(customsJSON, &c) == nil {
			rec.Customs = &c
		}
	}
	rec.BookingRef = deref(bookingRef)
	rec.TrackingNo = deref(trackingNo)
	rec.LabelContentType = deref(labelCT)
	rec.LabelZPL = deref(labelZPL)
	rec.PickupRef = deref(pickupRef)
	rec.PickupWindow = deref(pickupWin)
	rec.ManifestID = deref(manifestID)
	rec.ErrorMessage = deref(errMsg)
	rec.HasLabel = len(rec.LabelData) > 0
	rec.HasCustomsDoc = len(rec.CustomsDocPDF) > 0
	return rec, nil
}

func deref(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}

// ClaimForConfirmation atomically transitions pending_confirmation →
// confirming when (and only when) the token matches. This IS the gate: a
// wrong token, a replay, or a concurrent double-confirm all fail here,
// server-side, regardless of what any client claims.
func (s *Store) ClaimForConfirmation(ctx context.Context, id, token, actor string) error {
	tag, err := s.pool.Exec(ctx, `
		UPDATE bookings
		SET status = 'confirming', confirmed_by = $3, confirmed_at = now(), updated_at = now()
		WHERE id = $1 AND confirmation_token = $2 AND status = 'pending_confirmation'`,
		id, token, actor)
	if err != nil {
		return fmt.Errorf("claim booking for confirmation: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return ErrGate
	}
	return nil
}

// MarkBooked stores the carrier result + label + customs doc.
func (s *Store) MarkBooked(ctx context.Context, id string, b carrier.Booking, label carrier.Label, zpl string, customsDoc []byte) error {
	_, err := s.pool.Exec(ctx, `
		UPDATE bookings SET
			status = 'booked', booking_ref = $2, tracking_no = $3, booked_at = now(),
			label_content_type = NULLIF($4, ''), label_data = $5, label_zpl = NULLIF($6, ''),
			customs_doc_pdf = $7, error_message = NULL, updated_at = now()
		WHERE id = $1`,
		id, b.BookingRef, b.TrackingNo, label.ContentType, nullableBytes(label.Data), zpl, nullableBytes(customsDoc))
	if err != nil {
		return fmt.Errorf("mark booked: %w", err)
	}
	return nil
}

func nullableBytes(b []byte) any {
	if len(b) == 0 {
		return nil
	}
	return b
}

// MarkFailed records a carrier booking failure (terminal for this attempt;
// the user creates a new booking to retry — keeps the audit trail linear).
func (s *Store) MarkFailed(ctx context.Context, id, message string) error {
	_, err := s.pool.Exec(ctx, `
		UPDATE bookings SET status = 'failed', error_message = $2, updated_at = now()
		WHERE id = $1`, id, message)
	return err
}

// MarkCancelled transitions a booked booking to cancelled.
func (s *Store) MarkCancelled(ctx context.Context, id string) error {
	tag, err := s.pool.Exec(ctx, `
		UPDATE bookings SET status = 'cancelled', updated_at = now()
		WHERE id = $1 AND status IN ('booked', 'pending_confirmation')`, id)
	if err != nil {
		return err
	}
	if tag.RowsAffected() == 0 {
		return ErrGate
	}
	return nil
}

// SetPickup stores the pickup confirmation on a booking.
func (s *Store) SetPickup(ctx context.Context, id string, p carrier.Pickup) error {
	window := p.TimeFrom + "-" + p.TimeTo
	_, err := s.pool.Exec(ctx, `
		UPDATE bookings SET pickup_ref = $2, pickup_date = $3, pickup_window = $4, updated_at = now()
		WHERE id = $1`, id, p.PickupRef, p.Date, window)
	return err
}

// ListBookings returns recent bookings, optionally filtered by status.
func (s *Store) ListBookings(ctx context.Context, status string, limit int) ([]Record, error) {
	if limit <= 0 || limit > 200 {
		limit = 50
	}
	rows, err := s.pool.Query(ctx, `
		SELECT id, status, quote_ref, carrier_code, carrier_name, service_name,
		       price_amount_cents, price_currency, booking_ref, tracking_no,
		       booked_at, pickup_ref, manifest_id, booked_by, error_message,
		       (label_data IS NOT NULL), (customs_doc_pdf IS NOT NULL),
		       created_at, updated_at
		FROM bookings
		WHERE ($1 = '' OR status = $1)
		ORDER BY created_at DESC
		LIMIT $2`, status, limit)
	if err != nil {
		return nil, fmt.Errorf("list bookings: %w", err)
	}
	defer rows.Close()

	out := []Record{}
	for rows.Next() {
		var (
			rec        Record
			bookingRef *string
			trackingNo *string
			pickupRef  *string
			manifestID *string
			errMsg     *string
		)
		if err := rows.Scan(
			&rec.ID, &rec.Status, &rec.QuoteRef, &rec.CarrierCode, &rec.CarrierName,
			&rec.ServiceName, &rec.Price.AmountCents, &rec.Price.Currency,
			&bookingRef, &trackingNo, &rec.BookedAt, &pickupRef, &manifestID,
			&rec.BookedBy, &errMsg, &rec.HasLabel, &rec.HasCustomsDoc,
			&rec.CreatedAt, &rec.UpdatedAt,
		); err != nil {
			return nil, fmt.Errorf("scan booking: %w", err)
		}
		rec.BookingRef = deref(bookingRef)
		rec.TrackingNo = deref(trackingNo)
		rec.PickupRef = deref(pickupRef)
		rec.ManifestID = deref(manifestID)
		rec.ErrorMessage = deref(errMsg)
		out = append(out, rec)
	}
	return out, rows.Err()
}

// UpsertTrackingEvents persists carrier tracking events idempotently.
func (s *Store) UpsertTrackingEvents(ctx context.Context, bookingID string, events []carrier.TrackingEvent) error {
	for _, ev := range events {
		if _, err := s.pool.Exec(ctx, `
			INSERT INTO tracking_events (booking_id, status, description, occurred_at)
			VALUES ($1, $2, $3, $4)
			ON CONFLICT (booking_id, status, occurred_at) DO NOTHING`,
			bookingID, ev.Status, ev.Description, ev.OccurredAt); err != nil {
			return fmt.Errorf("upsert tracking event: %w", err)
		}
	}
	return nil
}

// ListTrackingEvents returns persisted events oldest-first.
func (s *Store) ListTrackingEvents(ctx context.Context, bookingID string) ([]carrier.TrackingEvent, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT status, description, occurred_at FROM tracking_events
		WHERE booking_id = $1 ORDER BY occurred_at ASC`, bookingID)
	if err != nil {
		return nil, fmt.Errorf("list tracking events: %w", err)
	}
	defer rows.Close()
	out := []carrier.TrackingEvent{}
	for rows.Next() {
		var ev carrier.TrackingEvent
		if err := rows.Scan(&ev.Status, &ev.Description, &ev.OccurredAt); err != nil {
			return nil, err
		}
		out = append(out, ev)
	}
	return out, rows.Err()
}

// Audit appends one immutable audit entry.
func (s *Store) Audit(ctx context.Context, bookingID, action, actor string, details map[string]any) {
	payload, _ := json.Marshal(details)
	// Best-effort by design at the call sites; failures are logged by callers.
	_, _ = s.pool.Exec(ctx, `
		INSERT INTO booking_audit_log (booking_id, action, actor, details)
		VALUES ($1, $2, $3, $4)`, bookingID, action, actor, payload)
}

// AuditTrail lists a booking's audit entries oldest-first.
func (s *Store) AuditTrail(ctx context.Context, bookingID string) ([]map[string]any, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT action, actor, details, created_at FROM booking_audit_log
		WHERE booking_id = $1 ORDER BY created_at ASC`, bookingID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []map[string]any{}
	for rows.Next() {
		var (
			action, actor string
			details       []byte
			at            time.Time
		)
		if err := rows.Scan(&action, &actor, &details, &at); err != nil {
			return nil, err
		}
		entry := map[string]any{"action": action, "actor": actor, "at": at}
		var d map[string]any
		if json.Unmarshal(details, &d) == nil && len(d) > 0 {
			entry["details"] = d
		}
		out = append(out, entry)
	}
	return out, rows.Err()
}

// CreateManifest groups every unmanifested booked booking for a carrier into
// a manifest row and stamps them. Returns the manifest id and the included
// booking records.
func (s *Store) CreateManifest(ctx context.Context, carrierCode, actor string, doc []byte) (string, []Record, error) {
	var id string
	err := s.pool.QueryRow(ctx, `
		INSERT INTO manifests (carrier_code, manifest_date, created_by, document_pdf)
		VALUES ($1, CURRENT_DATE, $2, $3)
		ON CONFLICT (carrier_code, manifest_date)
		DO UPDATE SET created_by = EXCLUDED.created_by
		RETURNING id`, carrierCode, actor, nullableBytes(doc)).Scan(&id)
	if err != nil {
		return "", nil, fmt.Errorf("insert manifest: %w", err)
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE bookings SET manifest_id = $1, updated_at = now()
		WHERE carrier_code = $2 AND status = 'booked' AND manifest_id IS NULL`, id, carrierCode)
	if err != nil {
		return "", nil, fmt.Errorf("stamp manifest bookings: %w", err)
	}
	_, err = s.pool.Exec(ctx, `
		UPDATE manifests SET booking_count = booking_count + $2 WHERE id = $1`, id, tag.RowsAffected())
	if err != nil {
		return "", nil, fmt.Errorf("update manifest count: %w", err)
	}
	rows, err := s.pool.Query(ctx, `
		SELECT id, booking_ref, tracking_no, service_name FROM bookings WHERE manifest_id = $1`, id)
	if err != nil {
		return "", nil, err
	}
	defer rows.Close()
	included := []Record{}
	for rows.Next() {
		var rec Record
		var bookingRef, trackingNo *string
		if err := rows.Scan(&rec.ID, &bookingRef, &trackingNo, &rec.ServiceName); err != nil {
			return "", nil, err
		}
		rec.BookingRef = deref(bookingRef)
		rec.TrackingNo = deref(trackingNo)
		included = append(included, rec)
	}
	return id, included, rows.Err()
}

// SetManifestDocument stores the rendered manifest PDF.
func (s *Store) SetManifestDocument(ctx context.Context, id string, doc []byte) error {
	_, err := s.pool.Exec(ctx, `UPDATE manifests SET document_pdf = $2 WHERE id = $1`, id, nullableBytes(doc))
	return err
}

// GetManifestDocument returns the manifest PDF.
func (s *Store) GetManifestDocument(ctx context.Context, id string) ([]byte, error) {
	var doc []byte
	err := s.pool.QueryRow(ctx, `SELECT document_pdf FROM manifests WHERE id = $1`, id).Scan(&doc)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return doc, nil
}
