package quoteengine

import (
	"context"
	"encoding/json"
	"net/http"
	"sort"
	"time"

	"github.com/go-playground/validator/v10"

	"shipping-core/internal/carrier"
)

var validate = validator.New()

// ReliabilityScorer is the subset of internal/reliability's Store this
// package depends on — defined at the consumer so quoteengine never
// couples to the DB pool or the reliability package's internals directly.
type ReliabilityScorer interface {
	ScoreMap(ctx context.Context) (map[string]float64, error)
}

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
// Consumers: the Verevon integration catalog (shipping provider readiness),
// the Model Plane shipping tools, and the future shipping page.
type carrierDTO struct {
	Code           string     `json:"code"`
	Name           string     `json:"name"`
	Segment        string     `json:"segment"`
	Mode           string     `json:"mode"`
	IsMock         bool       `json:"is_mock"`
	VerifiedAt     *time.Time `json:"verified_at"`
	DegradedReason string     `json:"degraded_reason,omitempty"`
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
				Code:           info.Code,
				Name:           info.Name,
				Segment:        string(info.Segment),
				Mode:           string(info.Mode),
				IsMock:         info.Mode == carrier.ModeMock,
				VerifiedAt:     info.VerifiedAt,
				DegradedReason: info.DegradedReason,
			})
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(resp)
	}
}

// Handler returns an http.HandlerFunc that validates the request body,
// fans it out via engine, and responds with quotes sorted cheapest-first
// plus any per-carrier errors — a timed-out or failing carrier is reported,
// never silently dropped. When scorer is non-nil, each quote's
// ReliabilityScore (F8) is populated from real booking/tracking history;
// a nil scorer (or a carrier below the minimum sample size) leaves it nil
// rather than fabricating a number.
func Handler(engine *Engine, scorer ReliabilityScorer) http.HandlerFunc {
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
		AnnotateReliability(r.Context(), scorer, resp.Quotes)

		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(resp)
	}
}

// AnnotateReliability sets ReliabilityScore on each quote from scorer's
// per-carrier map. Best-effort: a scorer error just leaves every quote's
// score nil. Exported so the recommend package (F5) can annotate the same
// quotes it hands to the model, not just the plain /api/quotes response.
func AnnotateReliability(ctx context.Context, scorer ReliabilityScorer, quotes []carrier.Quote) {
	if scorer == nil {
		return
	}
	scores, err := scorer.ScoreMap(ctx)
	if err != nil || len(scores) == 0 {
		return
	}
	for i := range quotes {
		if score, ok := scores[quotes[i].CarrierCode]; ok {
			s := score
			quotes[i].ReliabilityScore = &s
		}
	}
}

// reliabilityResponse is GET /api/carriers/reliability's body — every
// carrier with enough data to score, for transparency/debugging and the
// future shipping page (not just the opaque per-quote annotation above).
type reliabilityResponse struct {
	Carriers []reliabilityEntry `json:"carriers"`
}

type reliabilityEntry struct {
	CarrierCode string  `json:"carrier_code"`
	OnTimeRate  float64 `json:"on_time_rate"`
	SampleSize  int     `json:"sample_size"`
}

// ScoreEntry mirrors internal/reliability.CarrierScore's fields.
// quoteengine defines its own copy (rather than importing the reliability
// package) so it keeps depending only on the minimal shape it needs; main.go
// (which already imports both) adapts between them via ScoresFunc.
type ScoreEntry struct {
	CarrierCode string
	OnTimeRate  float64
	SampleSize  int
}

// ScoresFunc adapts a concrete scores provider (internal/reliability.Store)
// to what ReliabilityHandler needs, without quoteengine importing it.
type ScoresFunc func(ctx context.Context) ([]ScoreEntry, error)

// ReliabilityHandler serves GET /api/carriers/reliability.
func ReliabilityHandler(scores ScoresFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		resp := reliabilityResponse{Carriers: []reliabilityEntry{}}
		if entries, err := scores(r.Context()); err == nil {
			for _, sc := range entries {
				resp.Carriers = append(resp.Carriers, reliabilityEntry{
					CarrierCode: sc.CarrierCode, OnTimeRate: sc.OnTimeRate, SampleSize: sc.SampleSize,
				})
			}
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(resp)
	}
}
