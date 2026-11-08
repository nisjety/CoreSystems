package quarryotel

import (
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
)

// NewTracerProvider builds a provider with the same ZDR sampler, span processor
// and redacting exporter as Init, over any exporter and registry. Spans are
// handed to exp synchronously at span end. It is for in-process wiring and tests
// (a recording exporter), where Init's OTLP exporter and global state are not wanted.
func NewTracerProvider(exp sdktrace.SpanExporter, registry *TraceRegistry, exportOn bool, opts ...sdktrace.TracerProviderOption) *sdktrace.TracerProvider {
	all := []sdktrace.TracerProviderOption{
		sdktrace.WithSampler(NewZdrSampler(registry, exportOn)),
		sdktrace.WithSpanProcessor(newZdrProcessor(
			sdktrace.NewSimpleSpanProcessor(redactingExporter{SpanExporter: exp}),
			registry,
		)),
	}
	return sdktrace.NewTracerProvider(append(all, opts...)...)
}
