// Package sse exposes the Model Gateway SSE bridge for orchestration run events.
// Path: GET /v1/runs/{id}/events. Wire format: text/event-stream with
// `data: <json>\n\n` frames where the JSON payload is the protojson encoding
// of an OrchestrationEvent.
package sse

import (
	"errors"
	"io"
	"log/slog"
	"net/http"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/protobuf/encoding/protojson"
)

// Handler proxies an OrchestrationCoreService.StreamRunEvents gRPC stream to
// SSE clients. A nil client is allowed (returns 503 on every request) so that
// the gateway can boot without orchestrator-core in dev environments.
type Handler struct {
	client    mpv1.OrchestrationCoreServiceClient
	logger    *slog.Logger
	marshaler protojson.MarshalOptions
}

// New returns a Handler. When client is nil the handler responds 503 Service
// Unavailable; this matches the proxy.Proxy nil-degraded behaviour.
func New(client mpv1.OrchestrationCoreServiceClient, logger *slog.Logger) *Handler {
	if logger == nil {
		logger = slog.Default()
	}
	return &Handler{
		client: client,
		logger: logger,
		marshaler: protojson.MarshalOptions{
			UseProtoNames:   true,
			EmitUnpopulated: false,
		},
	}
}

func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		w.Header().Set("Allow", http.MethodGet)
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	runID, ok := parseRunID(r.URL.Path)
	if !ok {
		http.Error(w, "not found", http.StatusNotFound)
		return
	}

	if h.client == nil {
		http.Error(w, "orchestrator-core unavailable", http.StatusServiceUnavailable)
		return
	}

	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming unsupported", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	w.Header().Set("X-Accel-Buffering", "no")
	w.WriteHeader(http.StatusOK)
	flusher.Flush()

	stream, err := h.client.StreamRunEvents(r.Context(), &mpv1.StreamRunEventsRequest{RunId: runID})
	if err != nil {
		h.logger.Error("StreamRunEvents call failed", "run_id", runID, "err", err)
		writeError(w, flusher, "stream_open_failed")
		return
	}

	for {
		evt, err := stream.Recv()
		if err != nil {
			if errors.Is(err, io.EOF) || errors.Is(err, r.Context().Err()) {
				return
			}
			if r.Context().Err() != nil {
				return
			}
			h.logger.Error("Recv failed", "run_id", runID, "err", err)
			writeError(w, flusher, "stream_recv_failed")
			return
		}

		payload, err := h.marshaler.Marshal(evt)
		if err != nil {
			h.logger.Error("marshal failed", "run_id", runID, "err", err)
			continue
		}

		if _, err := w.Write([]byte("data: ")); err != nil {
			return
		}
		if _, err := w.Write(payload); err != nil {
			return
		}
		if _, err := w.Write([]byte("\n\n")); err != nil {
			return
		}
		flusher.Flush()
	}
}

// parseRunID extracts {id} from /v1/runs/{id}/events.
func parseRunID(path string) (string, bool) {
	rest := strings.TrimPrefix(path, "/v1/runs/")
	if rest == path {
		return "", false
	}
	parts := strings.Split(rest, "/")
	if len(parts) != 2 || parts[0] == "" || parts[1] != "events" {
		return "", false
	}
	return parts[0], true
}

func writeError(w http.ResponseWriter, flusher http.Flusher, code string) {
	_, _ = w.Write([]byte("event: error\ndata: {\"code\":\"" + code + "\"}\n\n"))
	flusher.Flush()
}
