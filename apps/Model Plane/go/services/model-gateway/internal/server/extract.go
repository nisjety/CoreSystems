// Fetch + ExtractStructured RPCs for model-gateway.
//
// These two RPCs replace the dead Python tools (web_fetch +
// extract_structured) from Model Plane v2. They use the canonical v1
// surface: a Go Quarry client (pkg/quarry) for the fetch step and
// inference-core's existing `structured_output_schema` path for the LLM
// coercion step — no new RPC on inference-core, no provider changes.
//
// Wire flow for ExtractStructured:
//
//	caller -> ModelGateway.ExtractStructured
//	          |
//	          +-- Quarry /v1/scrape (full-fat fetch: JS, TLS fp, charset, ...)
//	          |
//	          +-- inference-core.Infer (with structured_output_schema)
//	          |
//	          v
//	      structured JSON response
package server

import (
	"context"
	"fmt"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/quarry"
)

// Maximum page content (markdown chars) we forward to inference-core.
// Tuned to stay well below the 128k-context floor that every in-use
// model shares, leaving headroom for the schema + system prompt +
// caller instructions.
const maxExtractContentChars = 80_000

// Fetch is a pure pass-through to Quarry. Useful for callers that just
// want clean markdown + metadata without invoking the LLM (e.g. agent
// tool dispatchers, scheduled crawl previews).
func (s *Server) Fetch(ctx context.Context, req *mpv1.FetchRequest) (*mpv1.FetchResponse, error) {
	if s.quarry == nil || !s.quarry.Available() {
		// Quarry not wired (missing QUARRY_EDGE_URL at startup). This
		// is intentionally Unimplemented rather than FailedPrecondition
		// so callers can fall back to a simpler fetch path without
		// caring whether the gateway just isn't configured yet.
		return nil, status.Error(codes.Unimplemented, "quarry edge not configured")
	}
	if req.GetUrl() == "" {
		return nil, status.Error(codes.InvalidArgument, "url is required")
	}

	scrapeReq := quarry.ScrapeRequest{
		URL:         req.GetUrl(),
		OrgID:       req.GetOrgId(),
		Render:      renderHintsFromProto(req.GetRender()),
		PreferHTTP3: req.GetPreferHttp3(),
	}
	result, err := s.quarry.Scrape(ctx, scrapeReq)
	if err != nil {
		return nil, quarryErrToStatus(err)
	}

	return &mpv1.FetchResponse{
		RequestId:   req.GetRequestId(),
		Url:         result.URL,
		FinalUrl:    result.FinalURL,
		Status:      int32(result.Status),
		ContentType: result.ContentType,
		Title:       result.Title,
		Markdown:    result.Markdown,
		Text:        result.Text,
		Fingerprint: result.Fingerprint,
		Language:    result.Language,
	}, nil
}

// ExtractStructured fetches a URL and coerces the page content into a
// caller-supplied JSON schema. The gateway handles fetch; the LLM
// coercion is delegated to inference-core via its existing
// `structured_output_schema` parameter on InferRequest.
//
// We deliberately put the page markdown into the user message rather
// than a system message: provider adapters generally cache the user
// message less aggressively, which keeps each extraction independent.
func (s *Server) ExtractStructured(
	ctx context.Context,
	req *mpv1.ExtractStructuredRequest,
) (*mpv1.ExtractStructuredResponse, error) {
	if s.quarry == nil || !s.quarry.Available() {
		return nil, status.Error(codes.Unimplemented, "quarry edge not configured")
	}
	if s.inference == nil {
		// No inference-core client — without one we can fetch but
		// can't coerce. Distinct error so operators see the half-
		// configured state clearly.
		return nil, status.Error(codes.Unimplemented, "inference-core not configured")
	}
	if req.GetUrl() == "" {
		return nil, status.Error(codes.InvalidArgument, "url is required")
	}
	if req.GetSchemaJson() == "" {
		return nil, status.Error(codes.InvalidArgument, "schema_json is required")
	}

	// 1. Fetch via Quarry.
	scrape, err := s.quarry.Scrape(ctx, quarry.ScrapeRequest{
		URL:    req.GetUrl(),
		OrgID:  req.GetOrgId(),
		Render: renderHintsFromProto(req.GetRender()),
	})
	if err != nil {
		return nil, quarryErrToStatus(err)
	}

	content := scrape.Markdown
	if content == "" {
		content = scrape.Text
	}
	if content == "" {
		// Fetch succeeded but the page had no extractable text. Return
		// OK with an empty extracted_json + an error_message so the
		// caller sees a structured failure, not a gRPC error.
		return &mpv1.ExtractStructuredResponse{
			RequestId:         req.GetRequestId(),
			Url:               req.GetUrl(),
			FinalUrl:          scrape.FinalURL,
			Title:             scrape.Title,
			SourceFingerprint: scrape.Fingerprint,
			ErrorMessage:      "no extractable content from page",
		}, nil
	}
	if len(content) > maxExtractContentChars {
		content = content[:maxExtractContentChars]
	}

	// 2. Build the inference call. We use a single user message that
	// embeds the page content + instructions; structured_output_schema
	// makes the provider enforce the shape. This is the same coercion
	// path used by Invoke's existing `structured_output_schema` field.
	systemPrompt := "You extract structured data from web pages. " +
		"Read the supplied page content carefully. " +
		"For any field where the page does not contain a clear answer, " +
		"omit the field — do not invent values. " +
		"Return only a JSON object matching the schema."
	if req.GetInstructions() != "" {
		systemPrompt += "\n\nAdditional caller instructions:\n" + req.GetInstructions()
	}

	userPrompt := fmt.Sprintf(
		"URL: %s\n\n--- PAGE CONTENT ---\n%s%s\n--- END PAGE CONTENT ---\n\n"+
			"Extract the requested fields per the JSON schema.",
		req.GetUrl(),
		titleHeader(scrape.Title),
		content,
	)

	inferReq := &mpv1.InferRequest{
		RequestId:              req.GetRequestId(),
		OrgId:                  req.GetOrgId(),
		Model:                  req.GetModel(),
		ProviderHint:           req.GetProvider(),
		Messages:               []*mpv1.ChatMessage{
			{Role: "system", Content: systemPrompt},
			{Role: "user", Content: userPrompt},
		},
		Temperature:            0.0,
		MaxTokens:              2048,
		StructuredOutputSchema: req.GetSchemaJson(),
		Zdr:                    req.GetZdr(),
	}

	inferResp, err := s.inference.Infer(ctx, inferReq)
	if err != nil {
		// LLM call failed. Return OK with structured error so callers
		// see the page-fetch fingerprint + a typed failure.
		return &mpv1.ExtractStructuredResponse{
			RequestId:         req.GetRequestId(),
			Url:               req.GetUrl(),
			FinalUrl:          scrape.FinalURL,
			Title:             scrape.Title,
			SourceFingerprint: scrape.Fingerprint,
			ErrorMessage:      fmt.Sprintf("inference failed: %v", err),
		}, nil
	}

	return &mpv1.ExtractStructuredResponse{
		RequestId:         req.GetRequestId(),
		Url:               req.GetUrl(),
		FinalUrl:          scrape.FinalURL,
		Title:             scrape.Title,
		SourceFingerprint: scrape.Fingerprint,
		ExtractedJson:     inferResp.GetContent(),
		ModelUsed:         inferResp.GetModelUsed(),
		InputTokens:       inferResp.GetInputTokens(),
		OutputTokens:      inferResp.GetOutputTokens(),
	}, nil
}

func titleHeader(title string) string {
	if title == "" {
		return ""
	}
	return "# " + title + "\n\n"
}

func renderHintsFromProto(p *mpv1.RenderHints) *quarry.RenderHints {
	if p == nil || p.GetWaitForSelector() == "" {
		return nil
	}
	return &quarry.RenderHints{
		WaitForSelector:  p.GetWaitForSelector(),
		WaitForTimeoutMS: int(p.GetWaitForTimeoutMs()),
	}
}

// quarryErrToStatus maps Quarry's typed error envelope onto gRPC status
// codes so callers can distinguish "client did something wrong"
// (InvalidArgument / FailedPrecondition) from "upstream is unavailable"
// (Unavailable) from "the URL is genuinely forbidden" (PermissionDenied).
//
// We intentionally pass the Quarry error code through in the message so
// operators can trace back to the runtime-side log without an extra hop.
func quarryErrToStatus(err error) error {
	if err == quarry.ErrUnavailable {
		return status.Error(codes.Unimplemented, "quarry edge not configured")
	}
	qerr, ok := err.(*quarry.Error)
	if !ok {
		// Transport-level (DNS, refused, timeout). Bucket as Unavailable
		// so retries-with-backoff make sense.
		return status.Errorf(codes.Unavailable, "quarry: %v", err)
	}
	switch qerr.Code {
	case "BAD_REQUEST", "INVALID_ARGUMENT":
		return status.Error(codes.InvalidArgument, qerr.Message)
	case "SECURITY_BLOCKED", "FORBIDDEN":
		return status.Error(codes.PermissionDenied, qerr.Code+": "+qerr.Message)
	case "RATE_LIMITED":
		return status.Error(codes.ResourceExhausted, qerr.Message)
	case "TIMEOUT":
		return status.Error(codes.DeadlineExceeded, qerr.Message)
	case "UPSTREAM_BLOCKED":
		return status.Error(codes.Unavailable, qerr.Message)
	default:
		return status.Errorf(codes.Internal, "%s: %s", qerr.Code, qerr.Message)
	}
}
