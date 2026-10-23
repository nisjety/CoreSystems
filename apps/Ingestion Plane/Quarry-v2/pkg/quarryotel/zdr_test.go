package quarryotel

import (
	"context"
	"testing"

	"go.opentelemetry.io/otel/attribute"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"
)

// newZdrTestProvider wires the same sampler and processor as Init, over an in-memory exporter.
func newZdrTestProvider(t *testing.T, reg *TraceRegistry, exportOn bool) (*sdktrace.TracerProvider, *tracetest.InMemoryExporter) {
	t.Helper()
	exp := tracetest.NewInMemoryExporter()
	tp := sdktrace.NewTracerProvider(
		sdktrace.WithSampler(NewZdrSampler(reg, exportOn)),
		sdktrace.WithSpanProcessor(newZdrProcessor(
			sdktrace.NewSimpleSpanProcessor(redactingExporter{SpanExporter: exp}),
			reg,
		)),
	)
	t.Cleanup(func() { _ = tp.Shutdown(context.Background()) })
	return tp, exp
}

func TestZdrSamplerDropsFlaggedSpans(t *testing.T) {
	reg := NewTraceRegistry(16)
	tp, exp := newZdrTestProvider(t, reg, true)
	tracer := tp.Tracer("zdr-test")

	_, zdrSpan := tracer.Start(context.Background(), "request",
		trace.WithAttributes(attribute.Bool(ZdrAttribute, true)))
	zdrSpan.End()
	if n := len(exp.GetSpans()); n != 0 {
		t.Fatalf("a span created with zdr=true must not export, got %d", n)
	}

	ctx, clean := tracer.Start(context.Background(), "clean")
	clean.End()
	if n := len(exp.GetSpans()); n != 1 {
		t.Fatalf("a clean span exports, got %d", n)
	}

	reg.Flag(clean.SpanContext().TraceID())
	_, child := tracer.Start(ctx, "child")
	child.End()
	if n := len(exp.GetSpans()); n != 1 {
		t.Fatalf("a child of a flagged trace is dropped at start, got %d spans", n)
	}
}

func TestMarkZDRDropsTheWholeTraceAtEnd(t *testing.T) {
	reg := SharedRegistry()
	tp, exp := newZdrTestProvider(t, reg, true)
	tracer := tp.Tracer("zdr-test")

	ctx, root := tracer.Start(context.Background(), "root")
	MarkZDR(ctx)
	_, child := tracer.Start(ctx, "child")
	child.End()
	root.End()

	if n := len(exp.GetSpans()); n != 0 {
		t.Fatalf("after MarkZDR the whole trace exports nothing, got %d spans", n)
	}
	if !reg.Contains(root.SpanContext().TraceID()) {
		t.Fatal("MarkZDR flags the trace on the shared registry")
	}
}

func TestRegistryFifoEviction(t *testing.T) {
	reg := NewTraceRegistry(3)
	id := func(n int) trace.TraceID { return trace.TraceID([16]byte{15: byte(n)}) }

	for n := 1; n <= 4; n++ {
		reg.Flag(id(n))
	}
	if reg.Contains(id(1)) {
		t.Fatal("the oldest id is evicted at capacity")
	}
	if !reg.Contains(id(2)) || !reg.Contains(id(4)) {
		t.Fatal("the newer ids are held")
	}

	// A re-flag does not move an id to the back: 2 stays the oldest held.
	reg.Flag(id(2))
	reg.Flag(id(5))
	if reg.Contains(id(2)) {
		t.Fatal("FIFO, not LRU: a re-flag does not refresh position")
	}
	if !reg.Contains(id(3)) {
		t.Fatal("3 is held after 2 is evicted")
	}
	if reg.Len() != 3 || reg.Len() > reg.Capacity() {
		t.Fatalf("registry is bounded: len=%d cap=%d", reg.Len(), reg.Capacity())
	}
}

func TestRedactingExporterStripsContractKeys(t *testing.T) {
	exp := tracetest.NewInMemoryExporter()
	tp := sdktrace.NewTracerProvider(
		sdktrace.WithSpanProcessor(sdktrace.NewSimpleSpanProcessor(redactingExporter{SpanExporter: exp})),
	)
	t.Cleanup(func() { _ = tp.Shutdown(context.Background()) })

	_, span := tp.Tracer("zdr-test").Start(context.Background(), "scrape",
		trace.WithAttributes(
			attribute.String("url", "https://example.test/a"),
			attribute.String("authorization", "Bearer x"),
			attribute.Int("status_code", 200),
		))
	span.End()

	spans := exp.GetSpans()
	if len(spans) != 1 {
		t.Fatalf("want one exported span, got %d", len(spans))
	}
	keys := make([]string, 0, len(spans[0].Attributes))
	for _, kv := range spans[0].Attributes {
		keys = append(keys, string(kv.Key))
	}
	if len(keys) != 1 || keys[0] != "status_code" {
		t.Fatalf("only the safe key survives the exporter, got %v", keys)
	}
}
