package recommend

import (
	"encoding/json"
	"io"
	"net/http"
	"sort"

	"github.com/go-playground/validator/v10"

	"shipping-core/internal/carrier"
	"shipping-core/internal/events"
	"shipping-core/internal/quoteengine"
)

var validate = validator.New()

// Request/address/package DTOs mirror quoteengine's own (unexported)
// equivalents — each HTTP-facing package in shipping-core defines its own
// wire DTOs rather than importing another package's (see booking/http.go
// vs quoteengine/http.go's independent addressDTOs); this keeps each
// package's public HTTP contract self-contained.
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

type quoteRequestDTO struct {
	From    addressDTO `json:"from" validate:"required"`
	To      addressDTO `json:"to" validate:"required"`
	Package packageDTO `json:"package" validate:"required"`
	Segment string     `json:"segment" validate:"required,oneof=b2b b2c"`
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

type carrierErrorDTO struct {
	CarrierCode string `json:"carrier_code"`
	Message     string `json:"message"`
}

type recommendResponse struct {
	Quotes         []carrier.Quote   `json:"quotes"`
	Errors         []carrierErrorDTO `json:"errors,omitempty"`
	Recommendation Recommendation    `json:"recommendation"`
}

// Handler serves POST /api/quotes/recommend: fan out quotes exactly like
// /api/quotes (same validation, same cheapest-first sort, same F8
// reliability annotation), then ask Model Plane to reason over the
// resulting comparison table. publisher may be events.NoopPublisher{} —
// a successful recommendation publishes "recommendation.generated" for
// audit-core's wildcard subscription; an unavailable one publishes
// nothing (there's no decision to record).
func Handler(engine *quoteengine.Engine, scorer quoteengine.ReliabilityScorer, client ModelClient, publisher events.Publisher) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var dto quoteRequestDTO
		decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, 64*1024))
		decoder.DisallowUnknownFields()
		if err := decoder.Decode(&dto); err != nil {
			http.Error(w, `{"error":"invalid JSON body"}`, http.StatusBadRequest)
			return
		}
		if decoder.Decode(&struct{}{}) != io.EOF {
			http.Error(w, `{"error":"exactly one quote request is required"}`, http.StatusBadRequest)
			return
		}
		if err := validate.Struct(dto); err != nil {
			http.Error(w, `{"error":"`+err.Error()+`"}`, http.StatusBadRequest)
			return
		}

		domainReq := dto.toDomain()
		results := engine.GetQuotes(r.Context(), domainReq)
		resp := recommendResponse{Quotes: []carrier.Quote{}}
		for _, res := range results {
			if res.Err != nil {
				resp.Errors = append(resp.Errors, carrierErrorDTO{CarrierCode: res.CarrierCode, Message: res.Err.Error()})
				continue
			}
			resp.Quotes = append(resp.Quotes, res.Quotes...)
		}
		sort.Slice(resp.Quotes, func(i, j int) bool {
			return resp.Quotes[i].Price.AmountCents < resp.Quotes[j].Price.AmountCents
		})
		quoteengine.AnnotateReliability(r.Context(), scorer, resp.Quotes)
		resp.Recommendation = Recommend(r.Context(), client, domainReq, resp.Quotes)
		if resp.Recommendation.Available && publisher != nil {
			_ = publisher.Publish(r.Context(), events.Event{
				Type: "recommendation.generated",
				Data: map[string]any{
					"recommended_carrier_code": resp.Recommendation.RecommendedCarrierCode,
					"confidence":               resp.Recommendation.Confidence,
					"model_used":               resp.Recommendation.ModelUsed,
					"quote_count":              len(resp.Quotes),
				},
			})
		}

		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(resp)
	}
}
