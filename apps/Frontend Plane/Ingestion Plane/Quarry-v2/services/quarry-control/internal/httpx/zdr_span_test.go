package httpx

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"

	"github.com/triodelab/quarry-v2/pkg/quarryotel"
)

// zdrSpanRig is the real pkg/quarryotel sampler, processor and redacting exporter
// over a recording exporter, behind the ZdrSpan middleware.
func zdrSpanRig(t *testing.T, h http.Handler) (http.Handler, *tracetest.InMemoryExporter) {
	t.Helper()
	exp := tracetest.NewInMemoryExporter()
	tp := quarryotel.NewTracerProvider(exp, quarryotel.SharedRegistry(), true)
	t.Cleanup(func() { _ = tp.Shutdown(context.Background()) })
	return RequestID(ZdrSpanWith(tp)(h)), exp
}

func attrOf(attrs []attribute.KeyValue, key string) (attribute.Value, bool) {
	for _, kv := range attrs {
		if string(kv.Key) == key {
			return kv.Value, true
		}
	}
	return attribute.Value{}, false
}

func TestZdrSpanKeepsACleanRequestSpan(t *testing.T) {
	h, exp := zdrSpanRig(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusAccepted)
	}))
	req := httptest.NewRequest(http.MethodPost, "/v1/sources?q=secret-query", nil)
	req.Header.Set("X-Request-Id", "req_clean")
	h.ServeHTTP(httptest.NewRecorder(), req)

	spans := exp.GetSpans()
	if len(spans) != 1 {
		t.Fatalf("a clean request exports one span, got %d", len(spans))
	}
	s := spans[0]
	if s.Name != "POST" || s.SpanKind != trace.SpanKindServer {
		t.Fatalf("server span named by method, got %q kind %v", s.Name, s.SpanKind)
	}
	if v, ok := attrOf(s.Attributes, "status_code"); !ok || v.AsInt64() != http.StatusAccepted {
		t.Fatalf("status_code attribute missing or wrong: %v", s.Attributes)
	}
	if v, ok := attrOf(s.Attributes, "request_id"); !ok || v.AsString() != "req_clean" {
		t.Fatalf("request_id attribute missing or wrong: %v", s.Attributes)
	}
	for _, kv := range s.Attributes {
		if quarryotel.IsStrippedKey(string(kv.Key)) {
			t.Fatalf("the span must carry no sensitive key, found %q", kv.Key)
		}
		if kv.Value.Type() == attribute.STRING && kv.Value.AsString() == "secret-query" {
			t.Fatalf("the query leaked into %q", kv.Key)
		}
	}
	if _, ok := attrOf(s.Attributes, quarryotel.ZdrAttribute); ok {
		t.Fatal("a clean span is not marked zdr")
	}
}

func TestZdrSpanMarkZDRDropsTheRequestTrace(t *testing.T) {
	var traceID trace.TraceID
	h, exp := zdrSpanRig(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// What change_webhook.go and cycle23.go do once the ZDR posture is parsed.
		quarryotel.MarkZDR(r.Context())
		traceID = trace.SpanContextFromContext(r.Context()).TraceID()
		_, child := otel.GetTracerProvider().Tracer("t").Start(r.Context(), "child")
		child.End()
		w.WriteHeader(http.StatusBadRequest)
	}))
	h.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodPost, "/internal/change-webhook", nil))

	if !traceID.IsValid() {
		t.Fatal("the handler must see a valid request span: that is what makes MarkZDR work")
	}
	if n := len(exp.GetSpans()); n != 0 {
		t.Fatalf("a ZDR request exports no span, got %d", n)
	}
	if !quarryotel.SharedRegistry().Contains(traceID) {
		t.Fatal("MarkZDR flags the request trace on the shared registry")
	}
}

func TestZdrSpanSeesAValidSpanBeforeTheHandlerRuns(t *testing.T) {
	var valid bool
	h, _ := zdrSpanRig(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		valid = trace.SpanContextFromContext(r.Context()).IsValid()
	}))
	h.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/x", nil))
	if !valid {
		t.Fatal("no valid span in the request context")
	}
}

func TestZdrSpanAdoptsInboundTraceparent(t *testing.T) {
	prev := otel.GetTextMapPropagator()
	otel.SetTextMapPropagator(propagation.TraceContext{})
	t.Cleanup(func() { otel.SetTextMapPropagator(prev) })

	h, exp := zdrSpanRig(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {}))
	req := httptest.NewRequest(http.MethodGet, "/x", nil)
	req.Header.Set("traceparent", "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
	h.ServeHTTP(httptest.NewRecorder(), req)

	spans := exp.GetSpans()
	if len(spans) != 1 {
		t.Fatalf("one span, got %d", len(spans))
	}
	if got := spans[0].SpanContext.TraceID().String(); got != "0af7651916cd43dd8448eb211c80319c" {
		t.Fatalf("the span joins the inbound trace, got %s", got)
	}
	if got := spans[0].Parent.SpanID().String(); got != "b7ad6b7169203331" {
		t.Fatalf("the inbound span is the parent, got %s", got)
	}
}

func TestZdrSpanDefaultsToTheGlobalProviderAndIsSafeWithoutOne(t *testing.T) {
	// No exporter set up: the global provider is the no-op one and MarkZDR is a no-op.
	called := false
	h := ZdrSpan(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		quarryotel.MarkZDR(r.Context())
		called = true
		_, _ = w.Write([]byte("ok"))
	}))
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, httptest.NewRequest(http.MethodGet, "/x", nil))
	if !called || rec.Code != http.StatusOK || rec.Body.String() != "ok" {
		t.Fatalf("the middleware passes the request through untouched: %d %q", rec.Code, rec.Body.String())
	}
}
