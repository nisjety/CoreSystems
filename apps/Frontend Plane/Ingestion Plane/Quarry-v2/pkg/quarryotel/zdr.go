package quarryotel

// ZDR at the OTLP exporter (WS10.S8), the Go side of crates/quarry-edge/src/otel_zdr.rs.
// The same three pieces: a bounded FIFO registry of ZDR trace ids, a sampler that
// decides at span start, and a span processor plus exporter wrapper that decide
// at span end. Limits are the Rust side's: a span that ENDED before its trace was
// flagged is already in the batch and is not recalled.

import (
	"context"
	"sync"

	"go.opentelemetry.io/otel/attribute"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/trace"
)

// TraceRegistry is a bounded set of ZDR trace ids, evicted oldest-first (FIFO).
type TraceRegistry struct {
	mu      sync.Mutex
	cap     int
	order   []trace.TraceID
	members map[trace.TraceID]struct{}
}

// NewTraceRegistry returns a registry holding at most cap trace ids (at least 1).
func NewTraceRegistry(cap int) *TraceRegistry {
	if cap < 1 {
		cap = 1
	}
	return &TraceRegistry{cap: cap, members: make(map[trace.TraceID]struct{})}
}

// Capacity returns the bound on the number of trace ids held.
func (r *TraceRegistry) Capacity() int { return r.cap }

// Flag marks a trace as ZDR. Re-flagging a held id changes nothing; an invalid id is ignored.
func (r *TraceRegistry) Flag(id trace.TraceID) {
	if !id.IsValid() {
		return
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.members[id]; ok {
		return
	}
	for len(r.order) >= r.cap {
		delete(r.members, r.order[0])
		r.order = r.order[1:]
	}
	r.members[id] = struct{}{}
	r.order = append(r.order, id)
}

// Contains reports whether a trace id is flagged.
func (r *TraceRegistry) Contains(id trace.TraceID) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	_, ok := r.members[id]
	return ok
}

// Len returns the number of trace ids held (never more than Capacity).
func (r *TraceRegistry) Len() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.order)
}

var (
	sharedOnce     sync.Once
	sharedRegistry *TraceRegistry
)

// SharedRegistry is the process-wide registry, sized from RegistryCapEnv on first use.
func SharedRegistry() *TraceRegistry {
	sharedOnce.Do(func() {
		sharedRegistry = NewTraceRegistry(registryCapFromEnv())
	})
	return sharedRegistry
}

// MarkZDR flags the trace of the span in ctx and marks that span ZDR, so the whole
// trace is dropped from the moment of the call on. Call it where a ZDR posture is
// parsed after the span started. It is a no-op for a context without a valid span.
func MarkZDR(ctx context.Context) {
	span := trace.SpanFromContext(ctx)
	sc := span.SpanContext()
	if !sc.IsValid() {
		return
	}
	SharedRegistry().Flag(sc.TraceID())
	span.SetAttributes(attribute.Bool(ZdrAttribute, true))
}

// ZdrSampler decides at span start: it drops when export is off, when the trace is
// flagged, or when the span is created with zdr=true; otherwise ParentBased(AlwaysSample).
type ZdrSampler struct {
	registry *TraceRegistry
	exportOn bool
	inner    sdktrace.Sampler
}

// NewZdrSampler returns a sampler judging against registry. exportOn false drops everything.
func NewZdrSampler(registry *TraceRegistry, exportOn bool) sdktrace.Sampler {
	return &ZdrSampler{
		registry: registry,
		exportOn: exportOn,
		inner:    sdktrace.ParentBased(sdktrace.AlwaysSample()),
	}
}

// ShouldSample implements sdktrace.Sampler.
func (s *ZdrSampler) ShouldSample(p sdktrace.SamplingParameters) sdktrace.SamplingResult {
	if !s.exportOn || s.registry.Contains(p.TraceID) || hasZdrMark(p.Attributes) {
		return sdktrace.SamplingResult{
			Decision:   sdktrace.Drop,
			Tracestate: trace.SpanContextFromContext(p.ParentContext).TraceState(),
		}
	}
	return s.inner.ShouldSample(p)
}

// Description implements sdktrace.Sampler.
func (s *ZdrSampler) Description() string { return "quarryotel.ZdrSampler" }

// zdrProcessor drops ZDR spans and the spans of flagged traces at span end, and
// forwards the rest to next.
type zdrProcessor struct {
	next     sdktrace.SpanProcessor
	registry *TraceRegistry
}

func newZdrProcessor(next sdktrace.SpanProcessor, registry *TraceRegistry) sdktrace.SpanProcessor {
	return &zdrProcessor{next: next, registry: registry}
}

// OnStart implements sdktrace.SpanProcessor.
func (p *zdrProcessor) OnStart(parent context.Context, s sdktrace.ReadWriteSpan) {
	p.next.OnStart(parent, s)
}

// OnEnd implements sdktrace.SpanProcessor.
func (p *zdrProcessor) OnEnd(s sdktrace.ReadOnlySpan) {
	tid := s.SpanContext().TraceID()
	if hasZdrMark(s.Attributes()) {
		// A ZDR span drops, and so does the rest of its trace from here on.
		p.registry.Flag(tid)
		return
	}
	if p.registry.Contains(tid) {
		return
	}
	p.next.OnEnd(s)
}

// Shutdown implements sdktrace.SpanProcessor.
func (p *zdrProcessor) Shutdown(ctx context.Context) error { return p.next.Shutdown(ctx) }

// ForceFlush implements sdktrace.SpanProcessor.
func (p *zdrProcessor) ForceFlush(ctx context.Context) error { return p.next.ForceFlush(ctx) }

// redactingExporter strips the zdr_sensitive and never keys from every span it
// exports. It is the last stop before the wire.
type redactingExporter struct {
	sdktrace.SpanExporter
}

// ExportSpans implements sdktrace.SpanExporter.
func (e redactingExporter) ExportSpans(ctx context.Context, spans []sdktrace.ReadOnlySpan) error {
	wrapped := make([]sdktrace.ReadOnlySpan, 0, len(spans))
	for _, s := range spans {
		wrapped = append(wrapped, redactedSpan{ReadOnlySpan: s})
	}
	return e.SpanExporter.ExportSpans(ctx, wrapped)
}

// redactedSpan is a thin ReadOnlySpan that overrides only Attributes.
type redactedSpan struct {
	sdktrace.ReadOnlySpan
}

// Attributes implements sdktrace.ReadOnlySpan with the contract keys removed.
func (s redactedSpan) Attributes() []attribute.KeyValue {
	src := s.ReadOnlySpan.Attributes()
	out := make([]attribute.KeyValue, 0, len(src))
	for _, kv := range src {
		if IsStrippedKey(string(kv.Key)) {
			continue
		}
		out = append(out, kv)
	}
	return out
}

func hasZdrMark(attrs []attribute.KeyValue) bool {
	for _, kv := range attrs {
		if string(kv.Key) == ZdrAttribute && kv.Value.Type() == attribute.BOOL && kv.Value.AsBool() {
			return true
		}
	}
	return false
}
