package sse

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
)

// flushRecorder wraps httptest.ResponseRecorder and implements http.Flusher.
type flushRecorder struct {
	*httptest.ResponseRecorder
}

func (f *flushRecorder) Flush() {}

func newFlushRecorder() *flushRecorder {
	return &flushRecorder{ResponseRecorder: httptest.NewRecorder()}
}

// fakeStream implements grpc.ServerStreamingClient[mpv1.OrchestrationEvent].
type fakeStream struct {
	grpc.ClientStream
	events []*mpv1.OrchestrationEvent
	idx    int
}

func (f *fakeStream) Recv() (*mpv1.OrchestrationEvent, error) {
	if f.idx >= len(f.events) {
		return nil, io.EOF
	}
	e := f.events[f.idx]
	f.idx++
	return e, nil
}

// fakeClient implements mpv1.OrchestrationCoreServiceClient. Only StreamRunEvents
// is exercised; other methods are unused and will panic if invoked.
type fakeClient struct {
	mpv1.OrchestrationCoreServiceClient
	events []*mpv1.OrchestrationEvent
	err    error
}

func (c *fakeClient) StreamRunEvents(ctx context.Context, in *mpv1.StreamRunEventsRequest, opts ...grpc.CallOption) (grpc.ServerStreamingClient[mpv1.OrchestrationEvent], error) {
	if c.err != nil {
		return nil, c.err
	}
	return &fakeStream{events: c.events}, nil
}

func TestHandler_MethodNotAllowed(t *testing.T) {
	h := New(&fakeClient{}, nil)
	rec := newFlushRecorder()
	req := httptest.NewRequest(http.MethodPost, "/v1/runs/r1/events", nil)

	h.ServeHTTP(rec, req)

	if rec.Code != http.StatusMethodNotAllowed {
		t.Fatalf("got status %d, want 405", rec.Code)
	}
	if got := rec.Header().Get("Allow"); got != http.MethodGet {
		t.Fatalf("Allow header = %q, want GET", got)
	}
}

func TestHandler_BadPath(t *testing.T) {
	h := New(&fakeClient{}, nil)
	cases := []string{"/v1/foo", "/v1/runs/", "/v1/runs/abc", "/v1/runs/abc/events/extra"}
	for _, p := range cases {
		t.Run(p, func(t *testing.T) {
			rec := newFlushRecorder()
			req := httptest.NewRequest(http.MethodGet, p, nil)
			h.ServeHTTP(rec, req)
			if rec.Code != http.StatusNotFound {
				t.Fatalf("path %q got status %d, want 404", p, rec.Code)
			}
		})
	}
}

func TestHandler_NilClient(t *testing.T) {
	h := New(nil, nil)
	rec := newFlushRecorder()
	req := httptest.NewRequest(http.MethodGet, "/v1/runs/r1/events", nil)

	h.ServeHTTP(rec, req)

	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("got status %d, want 503", rec.Code)
	}
}

func TestHandler_StreamsEvents(t *testing.T) {
	client := &fakeClient{
		events: []*mpv1.OrchestrationEvent{
			{Event: &mpv1.OrchestrationEvent_PlanTransitioned_{PlanTransitioned: &mpv1.OrchestrationEvent_PlanTransitioned{PlanId: "p1", RunId: "r1"}}},
			{Event: &mpv1.OrchestrationEvent_PlanTransitioned_{PlanTransitioned: &mpv1.OrchestrationEvent_PlanTransitioned{PlanId: "p2", RunId: "r1"}}},
		},
	}
	h := New(client, nil)
	rec := newFlushRecorder()
	req := httptest.NewRequest(http.MethodGet, "/v1/runs/r1/events", nil)

	h.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("got status %d, want 200", rec.Code)
	}
	if ct := rec.Header().Get("Content-Type"); ct != "text/event-stream" {
		t.Fatalf("Content-Type = %q, want text/event-stream", ct)
	}
	if cc := rec.Header().Get("Cache-Control"); cc != "no-cache" {
		t.Fatalf("Cache-Control = %q, want no-cache", cc)
	}

	body := rec.Body.String()
	frames := strings.Count(body, "\n\n")
	if frames != 2 {
		t.Fatalf("expected 2 SSE frames, got %d (body=%q)", frames, body)
	}
	if !strings.Contains(body, "data: ") {
		t.Fatalf("body missing data: prefix; got %q", body)
	}
	// snake_case proto field names from UseProtoNames.
	if !strings.Contains(body, "run_id") {
		t.Fatalf("body missing snake_case run_id field; got %q", body)
	}
}
