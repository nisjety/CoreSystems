// Package grpcclient provides a factory for creating gRPC client connections
// to sibling services within the Model Plane.
package grpcclient

import (
	"context"
	"fmt"
	"log/slog"
	"time"

	"github.com/triodelab/model-plane/services/orchestrator-core/internal/servicecred"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// Clients holds gRPC connections to sibling services.
type Clients struct {
	SessionCore    *grpc.ClientConn
	InferenceCore  *grpc.ClientConn
	ExecutionCore  *grpc.ClientConn
	CapabilityCore *grpc.ClientConn
	SandboxManager *grpc.ClientConn
	BrowserBroker  *grpc.ClientConn
	LettaBridge    *grpc.ClientConn
}

// Options configures the gRPC client factory.
type Options struct {
	SessionCoreAddr    string
	InferenceCoreAddr  string
	ExecutionCoreAddr  string
	CapabilityCoreAddr string
	SandboxManagerAddr string
	BrowserBrokerAddr  string
	LettaBridgeAddr    string
	DialTimeout        time.Duration

	// Minters supplies the per-audience service-token minter used when a call
	// has no inbound credential to forward — the Temporal activity path. A nil
	// map keeps the pure-forwarding behavior on every connection.
	Minters servicecred.Minters

	// Logger receives the interceptor's re-mint warnings.
	Logger *slog.Logger
}

// Dial creates gRPC connections to all configured sibling services.
// Connections that fail to dial are logged as warnings but do not block startup,
// allowing the orchestrator to start even when some services are unavailable.
//
// Each connection gets interceptors bound to ITS OWN plane audience, because a
// minted token is audience-bound: one shared interceptor could carry only one
// audience's credential and would present it to every sibling, where the rest
// reject it. Connections with no audience (sandbox-manager, browser-broker) get
// forwarding only. Execution Core receives a service token only for the
// dedicated scheduled-step method; user-bound methods remain forwarding-only.
func Dial(ctx context.Context, opts Options) (*Clients, error) {
	if opts.DialTimeout == 0 {
		opts.DialTimeout = 5 * time.Second
	}
	logger := opts.Logger
	if logger == nil {
		logger = slog.Default()
	}

	clients := &Clients{}

	dial := func(addr, name, audience string) *grpc.ClientConn {
		dialCtx, cancel := context.WithTimeout(ctx, opts.DialTimeout)
		defer cancel()

		minter := opts.Minters.Get(audience)
		interceptor := servicecred.UnaryInterceptor(minter, logger)
		if name == "execution-core" {
			interceptor = servicecred.UnaryInterceptorForMethods(minter, logger, map[string]struct{}{
				"/model_plane.v1.ExecutionCore/ExecuteScheduledStep": {},
			})
		}
		dialOpts := []grpc.DialOption{
			grpc.WithTransportCredentials(insecure.NewCredentials()),
			// Proxies the caller's verified credential upstream; mints
			// orchestrator-core's own when there is none to forward.
			grpc.WithChainUnaryInterceptor(interceptor),
			grpc.WithChainStreamInterceptor(servicecred.StreamInterceptor()),
		}

		conn, err := grpc.DialContext(dialCtx, addr, dialOpts...)
		if err != nil {
			slog.Warn("failed to dial service", "service", name, "addr", addr, "error", err)
			return nil
		}
		slog.Info("connected to service",
			"service", name, "addr", addr, "service_token", minter != nil)
		return conn
	}

	clients.SessionCore = dial(opts.SessionCoreAddr, "session-core", servicecred.AudienceSessionCore)
	clients.InferenceCore = dial(opts.InferenceCoreAddr, "inference-core", servicecred.AudienceInferenceCore)
	clients.ExecutionCore = dial(opts.ExecutionCoreAddr, "execution-core", servicecred.AudienceExecutionCore)
	clients.CapabilityCore = dial(opts.CapabilityCoreAddr, "capability-core", servicecred.AudienceCapabilityCore)
	clients.SandboxManager = dial(opts.SandboxManagerAddr, "sandbox-manager", "")
	clients.BrowserBroker = dial(opts.BrowserBrokerAddr, "browser-broker", "")
	clients.LettaBridge = dial(opts.LettaBridgeAddr, "letta-bridge", servicecred.AudienceLettaBridge)

	return clients, nil
}

// Close gracefully closes all client connections.
func (c *Clients) Close() error {
	var firstErr error
	for _, conn := range []*grpc.ClientConn{
		c.SessionCore,
		c.InferenceCore,
		c.ExecutionCore,
		c.CapabilityCore,
		c.SandboxManager,
		c.BrowserBroker,
		c.LettaBridge,
	} {
		if conn == nil {
			continue
		}
		if err := conn.Close(); err != nil && firstErr == nil {
			firstErr = fmt.Errorf("close grpc conn: %w", err)
		}
	}
	return firstErr
}
