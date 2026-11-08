package httpx

import (
	"net/http"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/trace"
)

// spanTracerName is the instrumentation scope of the request span.
const spanTracerName = "quarry-control/httpx"

// ZdrSpan opens one server span per request (WS10.S8, D2-021). Before this
// middleware quarry-control had no request span, so quarryotel.MarkZDR (called
// from the change webhook and the source registration once a ZDR posture is
// parsed) found no valid span in the request context and did nothing. With the
// span in place MarkZDR flags the trace and marks this span zdr=true, and the
// sampler and span processor in pkg/quarryotel drop it.
//
// The span adopts an inbound W3C traceparent. It carries only fields from the
// telemetry field contract (method, request_id, status_code): never the URL,
// the query or a header, so a span that is exported leaks nothing. Without an
// exporter the global provider is the no-op one and the span is free.
func ZdrSpan(next http.Handler) http.Handler { return ZdrSpanWith(nil)(next) }

// ZdrSpanWith is ZdrSpan over an explicit tracer provider; nil means the global
// one, looked up per request so a provider set by quarryotel.Init is honoured.
func ZdrSpanWith(tp trace.TracerProvider) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return zdrSpan(tp, next)
	}
}

func zdrSpan(tp trace.TracerProvider, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		provider := tp
		if provider == nil {
			provider = otel.GetTracerProvider()
		}
		ctx := otel.GetTextMapPropagator().Extract(r.Context(), propagation.HeaderCarrier(r.Header))
		ctx, span := provider.Tracer(spanTracerName).Start(ctx, r.Method,
			trace.WithSpanKind(trace.SpanKindServer),
			trace.WithAttributes(attribute.String("method", r.Method)),
		)
		defer span.End()
		if id := RequestIDOf(r.Context()); id != "" {
			span.SetAttributes(attribute.String("request_id", id))
		}
		sr := &statusRecorder{ResponseWriter: w, status: http.StatusOK}
		next.ServeHTTP(sr, r.WithContext(ctx))
		span.SetAttributes(attribute.Int("status_code", sr.status))
	})
}
