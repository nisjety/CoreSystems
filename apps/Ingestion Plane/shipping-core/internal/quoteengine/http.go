package quoteengine

import (
	"encoding/json"
	"net/http"
	"sort"
	"strings"

	"github.com/go-playground/validator/v10"

	"shipping-core/internal/carrier"
)

var validate = validator.New()

// quoteRequestDTO is the wire shape for POST /api/quotes. Validation tags
// enforce the PRD's "validate every API boundary" NFR before any carrier
// is called.
type quoteRequestDTO struct {
	From    addressDTO `json:"from" validate:"required"`
	To      addressDTO `json:"to" validate:"required"`
	Package packageDTO `json:"package" validate:"required"`
	Segment string     `json:"segment" validate:"required,oneof=b2b b2c"`
}

type addressDTO struct {
	Name       string `json:"name" validate:"required"`
	Street     string `json:"street"`
	PostalCode string `json:"postal_code" validate:"required"`
	City       string `json:"city" validate:"required"`
	Country    string `json:"country" validate:"required,len=2"`
	IsBusiness bool   `json:"is_business"`
}

type packageDTO struct {
	WeightKg      float64 `json:"weight_kg" validate:"required,gt=0"`
	LengthCm      float64 `json:"length_cm" validate:"required,gt=0"`
	WidthCm       float64 `json:"width_cm" validate:"required,gt=0"`
	HeightCm      float64 `json:"height_cm" validate:"required,gt=0"`
	DangerousGood bool    `json:"dangerous_good"`
}

func (d quoteRequestDTO) toDomain() carrier.QuoteRequest {
	return carrier.QuoteRequest{
		From: carrier.Address{
			Name: d.From.Name, Street: d.From.Street, PostalCode: d.From.PostalCode,
			City: d.From.City, Country: d.From.Country, IsBusiness: d.From.IsBusiness,
		},
		To: carrier.Address{
			Name: d.To.Name, Street: d.To.Street, PostalCode: d.To.PostalCode,
			City: d.To.City, Country: d.To.Country, IsBusiness: d.To.IsBusiness,
		},
		Package: carrier.Package{
			WeightKg: d.Package.WeightKg, LengthCm: d.Package.LengthCm,
			WidthCm: d.Package.WidthCm, HeightCm: d.Package.HeightCm,
			DangerousGood: d.Package.DangerousGood,
		},
		Segment: carrier.Segment(d.Segment),
	}
}

type carrierError struct {
	CarrierCode string `json:"carrier_code"`
	Message     string `json:"message"`
}

type quoteResponse struct {
	Quotes []carrier.Quote `json:"quotes"`
	Errors []carrierError  `json:"errors,omitempty"`
}

// carrierDTO is one entry in GET /api/carriers: the adapter fleet as
// currently assembled (mock adapters carry the "mock-" code prefix; a real
// adapter replaces or extends them once its credentials are configured).
// Consumers: the Velion integration catalog (shipping provider readiness),
// the Model Plane shipping tools, and the future shipping page.
type carrierDTO struct {
	Code    string `json:"code"`
	Name    string `json:"name"`
	Segment string `json:"segment"`
	IsMock  bool   `json:"is_mock"`
}

type carriersResponse struct {
	Carriers []carrierDTO `json:"carriers"`
}

// CarriersHandler lists the registered carrier fleet.
func CarriersHandler(engine *Engine) http.HandlerFunc {
	return func(w http.ResponseWriter, _ *http.Request) {
		resp := carriersResponse{Carriers: []carrierDTO{}}
		for _, info := range engine.Carriers() {
			resp.Carriers = append(resp.Carriers, carrierDTO{
				Code:    info.Code,
				Name:    info.Name,
				Segment: string(info.Segment),
				IsMock:  strings.HasPrefix(info.Code, "mock-"),
			})
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(resp)
	}
}

// Handler returns an http.HandlerFunc that validates the request body,
// fans it out via engine, and responds with quotes sorted cheapest-first
// plus any per-carrier errors — a timed-out or failing carrier is reported,
// never silently dropped.
func Handler(engine *Engine) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var dto quoteRequestDTO
		if err := json.NewDecoder(r.Body).Decode(&dto); err != nil {
			http.Error(w, `{"error":"invalid JSON body"}`, http.StatusBadRequest)
			return
		}
		if err := validate.Struct(dto); err != nil {
			http.Error(w, `{"error":"`+err.Error()+`"}`, http.StatusBadRequest)
			return
		}

		results := engine.GetQuotes(r.Context(), dto.toDomain())
		resp := quoteResponse{Quotes: []carrier.Quote{}}
		for _, res := range results {
			if res.Err != nil {
				resp.Errors = append(resp.Errors, carrierError{CarrierCode: res.CarrierCode, Message: res.Err.Error()})
				continue
			}
			resp.Quotes = append(resp.Quotes, res.Quotes...)
		}
		sort.Slice(resp.Quotes, func(i, j int) bool {
			return resp.Quotes[i].Price.AmountCents < resp.Quotes[j].Price.AmountCents
		})

		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(resp)
	}
}
