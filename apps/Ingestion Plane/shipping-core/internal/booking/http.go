package booking

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/go-playground/validator/v10"

	shippingauth "shipping-core/internal/auth"
	"shipping-core/internal/carrier"
)

var validate = validator.New()

type addressDTO struct {
	Name       string `json:"name" validate:"required"`
	Street     string `json:"street"`
	PostalCode string `json:"postal_code" validate:"required"`
	City       string `json:"city" validate:"required"`
	Country    string `json:"country" validate:"required,len=2"`
	IsBusiness bool   `json:"is_business"`
}

func (d addressDTO) toDomain() carrier.Address {
	return carrier.Address{Name: d.Name, Street: d.Street, PostalCode: d.PostalCode, City: d.City, Country: d.Country, IsBusiness: d.IsBusiness}
}

type customsItemDTO struct {
	Description   string  `json:"description" validate:"required"`
	Quantity      int     `json:"quantity" validate:"required,gt=0"`
	ValueCents    int64   `json:"value_cents" validate:"required,gt=0"`
	Currency      string  `json:"currency" validate:"required,len=3"`
	WeightKg      float64 `json:"weight_kg" validate:"gte=0"`
	HSCode        string  `json:"hs_code"`
	OriginCountry string  `json:"origin_country" validate:"omitempty,len=2"`
}

type customsDTO struct {
	ContentsType string           `json:"contents_type" validate:"required,oneof=merchandise gift documents sample return"`
	Items        []customsItemDTO `json:"items" validate:"required,min=1,dive"`
	Incoterms    string           `json:"incoterms"`
	InvoiceNo    string           `json:"invoice_no"`
}

type createBookingDTO struct {
	QuoteRef    string `json:"quote_ref"`
	CarrierCode string `json:"carrier_code" validate:"required"`
	CarrierName string `json:"carrier_name"`
	ServiceName string `json:"service_name" validate:"required"`
	Price       struct {
		AmountCents int64  `json:"amount_cents" validate:"required,gt=0"`
		Currency    string `json:"currency" validate:"required,len=3"`
	} `json:"price" validate:"required"`
	From    addressDTO `json:"from" validate:"required"`
	To      addressDTO `json:"to" validate:"required"`
	Package struct {
		WeightKg      float64 `json:"weight_kg" validate:"required,gt=0"`
		LengthCm      float64 `json:"length_cm" validate:"required,gt=0"`
		WidthCm       float64 `json:"width_cm" validate:"required,gt=0"`
		HeightCm      float64 `json:"height_cm" validate:"required,gt=0"`
		DangerousGood bool    `json:"dangerous_good"`
	} `json:"package" validate:"required"`
	Customs        *customsDTO `json:"customs"`
	BookedBy       string      `json:"booked_by"` // deprecated: verified token actor is authoritative
	ApprovalID     string      `json:"approval_id"`
	ZDR            bool        `json:"zdr"`
	RetentionUntil string      `json:"retention_until" validate:"omitempty,datetime=2006-01-02T15:04:05Z07:00"`
	// EstimatedDelivery is optional: the quote's promised delivery date,
	// so this booking can later be scored for F8 reliability. Omit it and
	// the booking is simply excluded from scoring — never defaulted.
	EstimatedDelivery string `json:"estimated_delivery" validate:"omitempty,datetime=2006-01-02"`
}

type confirmDTO struct {
	ConfirmationToken string `json:"confirmation_token" validate:"required"`
	Actor             string `json:"actor"`
}

type pickupDTO struct {
	Date     string `json:"date" validate:"required,datetime=2006-01-02"`
	TimeFrom string `json:"time_from" validate:"required"`
	TimeTo   string `json:"time_to" validate:"required"`
	Note     string `json:"note"`
	Actor    string `json:"actor"`
}

type manifestDTO struct {
	CarrierCode string `json:"carrier_code" validate:"required"`
	Actor       string `json:"actor"`
}

// Routes mounts the booking lifecycle API onto r.
func Routes(r chi.Router, svc *Service, store *Store) {
	r.Post("/api/bookings", createHandler(svc))
	r.Get("/api/bookings", listHandler(store))
	r.Get("/api/bookings/{id}", getHandler(store))
	r.Post("/api/bookings/{id}/confirm", confirmHandler(svc))
	r.Post("/api/bookings/{id}/cancel", cancelHandler(svc))
	r.Get("/api/bookings/{id}/label", labelHandler(store))
	r.Get("/api/bookings/{id}/customs-document", customsDocHandler(store))
	r.Post("/api/bookings/{id}/pickup", pickupHandler(svc))
	r.Get("/api/bookings/{id}/tracking", trackingHandler(svc))
	r.Get("/api/bookings/{id}/audit", auditHandler(store))
	r.Post("/api/manifests", manifestHandler(svc))
	r.Get("/api/manifests/{id}/document", manifestDocHandler(store))
}

func createHandler(svc *Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		var dto createBookingDTO
		if !decodeAndValidate(w, r, &dto) {
			return
		}
		var customs *carrier.CustomsInfo
		if dto.Customs != nil {
			items := make([]carrier.CustomsItem, 0, len(dto.Customs.Items))
			for _, it := range dto.Customs.Items {
				items = append(items, carrier.CustomsItem{
					Description: it.Description, Quantity: it.Quantity, ValueCents: it.ValueCents,
					Currency: it.Currency, WeightKg: it.WeightKg, HSCode: it.HSCode, OriginCountry: it.OriginCountry,
				})
			}
			customs = &carrier.CustomsInfo{ContentsType: dto.Customs.ContentsType, Items: items, Incoterms: dto.Customs.Incoterms, InvoiceNo: dto.Customs.InvoiceNo}
		}
		var estimatedDelivery *time.Time
		if dto.EstimatedDelivery != "" {
			if parsed, perr := time.Parse("2006-01-02", dto.EstimatedDelivery); perr == nil {
				estimatedDelivery = &parsed
			}
		}
		var retentionUntil *time.Time
		if dto.RetentionUntil != "" {
			parsed, _ := time.Parse(time.RFC3339, dto.RetentionUntil)
			retentionUntil = &parsed
		}
		idempotencyKey := strings.TrimSpace(r.Header.Get("Idempotency-Key"))
		if idempotencyKey != "" && (len(idempotencyKey) < 8 || len(idempotencyKey) > 128) {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": "Idempotency-Key must be 8-128 characters"})
			return
		}
		result, err := svc.Create(r.Context(), CreateInput{
			OrgID:              principal.OrganizationID,
			BookedBy:           principal.ActorID,
			ActorPrincipalType: principal.PrincipalType,
			QuoteRef:           dto.QuoteRef,
			CarrierCode:        dto.CarrierCode,
			CarrierName:        dto.CarrierName,
			ServiceName:        dto.ServiceName,
			Price:              carrier.Money{AmountCents: dto.Price.AmountCents, Currency: dto.Price.Currency},
			From:               dto.From.toDomain(),
			To:                 dto.To.toDomain(),
			Package: carrier.Package{
				WeightKg: dto.Package.WeightKg, LengthCm: dto.Package.LengthCm,
				WidthCm: dto.Package.WidthCm, HeightCm: dto.Package.HeightCm,
				DangerousGood: dto.Package.DangerousGood,
			},
			Customs:           customs,
			ApprovalID:        strings.TrimSpace(dto.ApprovalID),
			IdempotencyKey:    idempotencyKey,
			ZDR:               dto.ZDR,
			RetentionUntil:    retentionUntil,
			EstimatedDelivery: estimatedDelivery,
		})
		if err != nil {
			writeErr(w, err)
			return
		}
		writeJSON(w, http.StatusCreated, result)
	}
}

func confirmHandler(svc *Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		var dto confirmDTO
		if !decodeAndValidate(w, r, &dto) {
			return
		}
		rec, err := svc.Confirm(r.Context(), principal.OrganizationID, chi.URLParam(r, "id"), dto.ConfirmationToken, principal.ActorID)
		if err != nil {
			writeErr(w, err)
			return
		}
		writeJSON(w, http.StatusOK, rec)
	}
}

func cancelHandler(svc *Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		if err := svc.Cancel(r.Context(), principal.OrganizationID, chi.URLParam(r, "id"), principal.ActorID); err != nil {
			writeErr(w, err)
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"cancelled": true})
	}
}

func getHandler(store *Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		rec, err := store.GetBooking(r.Context(), principal.OrganizationID, chi.URLParam(r, "id"))
		if err != nil {
			writeErr(w, err)
			return
		}
		writeJSON(w, http.StatusOK, rec)
	}
}

func listHandler(store *Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		recs, err := store.ListBookings(r.Context(), principal.OrganizationID, r.URL.Query().Get("status"), 50)
		if err != nil {
			writeErr(w, err)
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"bookings": recs})
	}
}

// labelHandler serves the stored label. Default: raw bytes (PDF). With
// ?format=zpl the ZPL text; with ?format=json a base64 envelope (what the
// Velion gateway proxies, since its JSON pipe cannot carry raw PDF bytes).
func labelHandler(store *Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		rec, err := store.GetBooking(r.Context(), principal.OrganizationID, chi.URLParam(r, "id"))
		if err != nil {
			writeErr(w, err)
			return
		}
		switch r.URL.Query().Get("format") {
		case "zpl":
			if rec.LabelZPL == "" {
				writeJSON(w, http.StatusNotFound, map[string]any{"error": "no ZPL label stored for this booking"})
				return
			}
			w.Header().Set("Content-Type", "text/plain; charset=utf-8")
			_, _ = w.Write([]byte(rec.LabelZPL))
		case "json":
			if len(rec.LabelData) == 0 {
				writeJSON(w, http.StatusNotFound, map[string]any{"error": "no label stored for this booking"})
				return
			}
			writeJSON(w, http.StatusOK, map[string]any{
				"content_type":   rec.LabelContentType,
				"filename":       "label-" + rec.ID + ".pdf",
				"content_base64": base64.StdEncoding.EncodeToString(rec.LabelData),
				"zpl":            rec.LabelZPL,
			})
		default:
			if len(rec.LabelData) == 0 {
				writeJSON(w, http.StatusNotFound, map[string]any{"error": "no label stored for this booking"})
				return
			}
			w.Header().Set("Content-Type", rec.LabelContentType)
			w.Header().Set("Content-Disposition", `attachment; filename="label-`+rec.ID+`.pdf"`)
			_, _ = w.Write(rec.LabelData)
		}
	}
}

func customsDocHandler(store *Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		rec, err := store.GetBooking(r.Context(), principal.OrganizationID, chi.URLParam(r, "id"))
		if err != nil {
			writeErr(w, err)
			return
		}
		if len(rec.CustomsDocPDF) == 0 {
			writeJSON(w, http.StatusNotFound, map[string]any{"error": "no customs document for this booking"})
			return
		}
		if r.URL.Query().Get("format") == "json" {
			writeJSON(w, http.StatusOK, map[string]any{
				"content_type":   "application/pdf",
				"filename":       "customs-" + rec.ID + ".pdf",
				"content_base64": base64.StdEncoding.EncodeToString(rec.CustomsDocPDF),
			})
			return
		}
		w.Header().Set("Content-Type", "application/pdf")
		w.Header().Set("Content-Disposition", `attachment; filename="customs-`+rec.ID+`.pdf"`)
		_, _ = w.Write(rec.CustomsDocPDF)
	}
}

func pickupHandler(svc *Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		var dto pickupDTO
		if !decodeAndValidate(w, r, &dto) {
			return
		}
		date, _ := time.Parse("2006-01-02", dto.Date)
		pickup, err := svc.SchedulePickup(r.Context(), principal.OrganizationID, chi.URLParam(r, "id"), principal.ActorID, carrier.PickupRequest{
			Date: date, TimeFrom: dto.TimeFrom, TimeTo: dto.TimeTo, Note: dto.Note,
		})
		if err != nil {
			writeErr(w, err)
			return
		}
		writeJSON(w, http.StatusOK, pickup)
	}
}

func trackingHandler(svc *Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		tracking, err := svc.Tracking(r.Context(), principal.OrganizationID, chi.URLParam(r, "id"))
		if err != nil {
			writeErr(w, err)
			return
		}
		writeJSON(w, http.StatusOK, tracking)
	}
}

func auditHandler(store *Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		trail, err := store.AuditTrail(r.Context(), principal.OrganizationID, chi.URLParam(r, "id"))
		if err != nil {
			writeErr(w, err)
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"audit": trail})
	}
}

func manifestHandler(svc *Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		var dto manifestDTO
		if !decodeAndValidate(w, r, &dto) {
			return
		}
		id, count, err := svc.BuildManifest(r.Context(), principal.OrganizationID, dto.CarrierCode, principal.ActorID)
		if err != nil {
			writeErr(w, err)
			return
		}
		writeJSON(w, http.StatusCreated, map[string]any{"manifest_id": id, "booking_count": count})
	}
}

func manifestDocHandler(store *Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		principal, ok := verifiedPrincipal(w, r)
		if !ok {
			return
		}
		doc, err := store.GetManifestDocument(r.Context(), principal.OrganizationID, chi.URLParam(r, "id"))
		if err != nil {
			writeErr(w, err)
			return
		}
		if len(doc) == 0 {
			writeJSON(w, http.StatusNotFound, map[string]any{"error": "manifest has no document"})
			return
		}
		w.Header().Set("Content-Type", "application/pdf")
		_, _ = w.Write(doc)
	}
}

func verifiedPrincipal(w http.ResponseWriter, r *http.Request) (shippingauth.Principal, bool) {
	principal, ok := shippingauth.PrincipalFromContext(r.Context())
	if !ok {
		writeJSON(w, http.StatusUnauthorized, map[string]any{"error": "verified identity is required"})
		return shippingauth.Principal{}, false
	}
	return principal, true
}

func decodeAndValidate(w http.ResponseWriter, r *http.Request, dto any) bool {
	if err := json.NewDecoder(r.Body).Decode(dto); err != nil {
		writeJSON(w, http.StatusBadRequest, map[string]any{"error": "invalid JSON body"})
		return false
	}
	if err := validate.Struct(dto); err != nil {
		writeJSON(w, http.StatusBadRequest, map[string]any{"error": err.Error()})
		return false
	}
	return true
}

func writeErr(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, ErrNotFound):
		writeJSON(w, http.StatusNotFound, map[string]any{"error": "not found"})
	case errors.Is(err, ErrGate):
		// 409: the gate rejected the transition (bad token, replay, or a
		// state that no longer permits it).
		writeJSON(w, http.StatusConflict, map[string]any{"error": "confirmation rejected: invalid token or booking is not awaiting confirmation"})
	case errors.Is(err, ErrIdempotencyConflict):
		writeJSON(w, http.StatusConflict, map[string]any{"error": "idempotency key was already used for a different booking request"})
	case errors.Is(err, ErrValidation):
		writeJSON(w, http.StatusBadRequest, map[string]any{"error": err.Error()})
	default:
		writeJSON(w, http.StatusBadGateway, map[string]any{"error": err.Error()})
	}
}

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}
