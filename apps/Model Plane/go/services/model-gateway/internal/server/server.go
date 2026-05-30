// Package server implements the model_plane.v1.ModelGateway gRPC service.
//
// model-gateway is the front-door entrypoint for the interactive invoke path:
//
//	model-gateway -> session-core -> inference-core -> execution-core
//
// Wiring is feature-flag-by-presence: if NewServer is constructed with a nil
// proxy, Invoke / InvokeStream return Unimplemented (preserving the v2
// fallback path); when a proxy is supplied, calls are forwarded to
// session-core + inference-core.
package server

import (
	"context"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/quarry"
	"github.com/triodelab/model-plane/services/model-gateway/internal/proxy"
)

// Server implements mpv1.ModelGatewayServer.
//
// Fields:
//   - proxy: session-core + inference-core fan-out for Invoke/InvokeStream.
//   - quarry: Quarry-v2 edge client for Fetch + ExtractStructured (Wave 9).
//   - inference: direct inference-core client for ExtractStructured (no
//     session/thread side-effects — extraction is stateless).
//
// All three are independently nil-able to keep partial-startup possible
// (the gateway can serve Fetch alone even when inference-core is down).
type Server struct {
	mpv1.UnimplementedModelGatewayServer
	proxy     *proxy.Proxy
	quarry    *quarry.Client
	inference proxy.InferenceClient
}

// NewServer constructs a Server. Pass nil for any dependency to put that
// RPC in Unimplemented mode; everything else keeps working. This mirrors
// the existing feature-flag-by-presence pattern.
//
// Variants exist on the struct directly because the constructor list was
// going to grow indefinitely (cost-core, capability-core, …); we keep
// NewServer minimal and let main.go set additional fields explicitly.
func NewServer(p *proxy.Proxy) *Server { return &Server{proxy: p} }

// SetQuarry wires the Quarry edge client used by Fetch + ExtractStructured.
// Pass nil to leave those RPCs returning Unimplemented.
func (s *Server) SetQuarry(c *quarry.Client) { s.quarry = c }

// SetInferenceClient wires a direct inference-core client for the
// ExtractStructured LLM-coercion call. We deliberately don't reuse
// `proxy.Proxy` here because ExtractStructured doesn't touch session-core
// (no thread, no run) — using the proxy would force a fake thread setup.
func (s *Server) SetInferenceClient(c proxy.InferenceClient) { s.inference = c }

// Invoke is the unary entry point for non-streaming invocations.
func (s *Server) Invoke(ctx context.Context, req *mpv1.InvokeRequest) (*mpv1.InvokeResponse, error) {
	if s.proxy == nil {
		return nil, status.Error(codes.Unimplemented, "downstream proxy not configured")
	}
	resp, err := s.proxy.Invoke(ctx, req)
	if err != nil {
		return nil, status.Error(codes.Internal, err.Error())
	}
	return resp, nil
}

// InvokeStream is the server-streaming entry point.
func (s *Server) InvokeStream(req *mpv1.InvokeRequest, stream mpv1.ModelGateway_InvokeStreamServer) error {
	if s.proxy == nil {
		return status.Error(codes.Unimplemented, "downstream proxy not configured")
	}
	if err := s.proxy.InvokeStream(req, stream); err != nil {
		return status.Error(codes.Internal, err.Error())
	}
	return nil
}

// Health reports the liveness of the gateway itself (not downstreams).
func (s *Server) Health(_ context.Context, _ *mpv1.HealthRequest) (*mpv1.HealthResponse, error) {
	return &mpv1.HealthResponse{Status: "OK"}, nil
}
