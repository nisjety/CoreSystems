// Package recommend implements F5: an AI-reasoned recommendation over a
// carrier comparison table. It renders the quotes (plus F8 reliability
// scores, when present) as text, asks Model Plane for a structured
// recommendation via a JSON schema, and returns an honest "unavailable"
// result rather than a fabricated pick when the model's output doesn't
// parse or Model Plane is unreachable — mirroring model-gateway's own
// recommend_plan handler, which parses defensively and never hard-fails
// the request, adapted here to never fabricate WHICH carrier to book.
package recommend

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"shipping-core/internal/carrier"
	"shipping-core/internal/modelplane"
)

// schema is the structured_output_schema sent to model-gateway's
// /v1/invoke — same plain JSON-Schema shape as model-gateway's own
// RECOMMEND_PLAN_SCHEMA (http_routes.rs), snake_case to match every other
// field in shipping-core's own API responses.
const schema = `{"type":"object","properties":{` +
	`"recommended_carrier_code":{"type":"string"},` +
	`"recommended_service_name":{"type":"string"},` +
	`"reasoning":{"type":"string"},` +
	`"confidence":{"type":"number"},` +
	`"tradeoffs":{"type":"array","items":{"type":"string"}}},` +
	`"required":["recommended_carrier_code","reasoning","confidence"]}`

// Recommendation is F5's structured output: which quote to book and why.
// Available is false when the model call failed or the response didn't
// parse — callers must check it before trusting the other fields; this
// package never fabricates a carrier pick to fill the gap.
type Recommendation struct {
	Environment            string    `json:"environment,omitempty"`
	IsMock                 bool      `json:"is_mock"`
	QuotedAt               time.Time `json:"quoted_at,omitempty"`
	PackageCount           int       `json:"package_count,omitempty"`
	Available              bool      `json:"available"`
	RecommendedCarrierCode string    `json:"recommended_carrier_code,omitempty"`
	RecommendedServiceName string    `json:"recommended_service_name,omitempty"`
	Reasoning              string    `json:"reasoning,omitempty"`
	Confidence             float64   `json:"confidence,omitempty"`
	Tradeoffs              []string  `json:"tradeoffs,omitempty"`
	ModelUsed              string    `json:"model_used,omitempty"`
	UnavailableReason      string    `json:"unavailable_reason,omitempty"`
}

// ModelClient is the subset of modelplane.Client this package depends on
// — satisfied by *modelplane.Client directly; a minimal interface here
// only for test doubles.
type ModelClient interface {
	Configured() bool
	Invoke(ctx context.Context, req modelplane.InvokeRequest) (modelplane.InvokeResponse, error)
}

// Recommend renders quotes as a comparison for the model, asks for a
// structured recommendation, and parses the result. Never returns a Go
// error for "no recommendation" cases (unconfigured, unreachable,
// unparseable) — those are reported honestly via Recommendation.Available
// / UnavailableReason so a quote response is never blocked by an AI
// feature being down.
func Recommend(ctx context.Context, client ModelClient, req carrier.QuoteRequest, quotes []carrier.Quote) Recommendation {
	if len(quotes) == 0 {
		return Recommendation{UnavailableReason: "no quotes to recommend from"}
	}
	if !client.Configured() {
		return Recommendation{UnavailableReason: "AI recommendation is not configured"}
	}

	resp, err := client.Invoke(ctx, modelplane.InvokeRequest{
		Content:                buildPrompt(req, quotes),
		Model:                  "verevon-balance",
		StructuredOutputSchema: schema,
	})
	if err != nil {
		return Recommendation{UnavailableReason: fmt.Sprintf("model plane call failed: %v", err)}
	}

	var parsed struct {
		RecommendedCarrierCode string   `json:"recommended_carrier_code"`
		RecommendedServiceName string   `json:"recommended_service_name"`
		Reasoning              string   `json:"reasoning"`
		Confidence             float64  `json:"confidence"`
		Tradeoffs              []string `json:"tradeoffs"`
	}
	if err := json.Unmarshal([]byte(strings.TrimSpace(resp.Content)), &parsed); err != nil {
		return Recommendation{UnavailableReason: fmt.Sprintf("model response did not parse as the expected schema: %v", err)}
	}
	if parsed.RecommendedCarrierCode == "" || parsed.Reasoning == "" {
		return Recommendation{UnavailableReason: "model response missing required fields"}
	}
	// Guard against the model naming a carrier that wasn't actually
	// quoted — a hallucinated pick is worse than none.
	var selected *carrier.Quote
	for _, q := range quotes {
		if q.CarrierCode == parsed.RecommendedCarrierCode && (parsed.RecommendedServiceName == "" || parsed.RecommendedServiceName == q.ServiceName) {
			selected = &q
			break
		}
	}
	if selected == nil {
		return Recommendation{UnavailableReason: fmt.Sprintf("model recommended carrier_code %q, which was not among the quoted options", parsed.RecommendedCarrierCode)}
	}

	return Recommendation{
		Environment:            selected.Environment,
		IsMock:                 selected.IsMock,
		QuotedAt:               selected.QuotedAt,
		PackageCount:           selected.PackageCount,
		Available:              true,
		RecommendedCarrierCode: parsed.RecommendedCarrierCode,
		RecommendedServiceName: selected.ServiceName,
		Reasoning:              parsed.Reasoning,
		Confidence:             parsed.Confidence,
		Tradeoffs:              parsed.Tradeoffs,
		ModelUsed:              resp.ModelUsed,
	}
}

// buildPrompt renders the shipment context and comparison table as plain
// text — same information shape as execution-core's shipping_tools.rs
// render_quotes, so both surfaces describe quotes identically.
func buildPrompt(req carrier.QuoteRequest, quotes []carrier.Quote) string {
	var b strings.Builder
	fmt.Fprintf(&b, "A customer wants to ship a package from %s, %s to %s, %s (recipient segment: %s).\n",
		req.From.City, req.From.Country, req.To.City, req.To.Country, req.Segment)
	fmt.Fprintf(&b, "Package: %.1f kg, %.0fx%.0fx%.0f cm.", req.Package.WeightKg, req.Package.LengthCm, req.Package.WidthCm, req.Package.HeightCm)
	if req.Package.DangerousGood {
		b.WriteString(" Contains dangerous goods.")
	}
	b.WriteString("\n\nAvailable shipping options:\n")
	for _, q := range quotes {
		fmt.Fprintf(&b, "- carrier_code=%s (%s), service=%s: %d.%02d %s",
			q.CarrierCode, q.CarrierName, q.ServiceName,
			q.Price.AmountCents/100, q.Price.AmountCents%100, q.Price.Currency)
		environment := q.Environment
		if environment == "" {
			environment = "unknown"
		}
		fmt.Fprintf(&b, ", environment=%s, package_count=%d, quoted_at=%s", environment, q.PackageCount, q.QuotedAt.Format(time.RFC3339))
		if q.TransitDays > 0 {
			fmt.Fprintf(&b, ", %d day(s) transit", q.TransitDays)
		}
		if !q.EstimatedDelivery.IsZero() {
			fmt.Fprintf(&b, ", ETA %s", q.EstimatedDelivery.Format("2006-01-02"))
		}
		if q.ReliabilityScore != nil {
			fmt.Fprintf(&b, ", on-time delivery history %.0f%%", *q.ReliabilityScore*100)
		}
		if len(q.Features) > 0 {
			fmt.Fprintf(&b, " [%s]", strings.Join(q.Features, ", "))
		}
		b.WriteString("\n")
	}
	b.WriteString("\nRecommend the single best option using carrier_code exactly as given above. " +
		"Copy the exact quoted service name. Production quotes are live estimates, while sandbox/mock prices are test data and unknown environment is unverified; state this explicitly and prefer production estimates for real shipments. These prices cover the quoted package count only, never an unquoted larger shipment. " +
		"Weigh price, speed, and on-time delivery history (when present) against what this shipment " +
		"needs — do not default to the cheapest option if a modest price difference buys materially " +
		"better reliability or speed. Explain your reasoning in 2-3 sentences, and note any real " +
		"tradeoffs the customer is accepting.")
	return b.String()
}
