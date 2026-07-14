// Package grpcclient provides a factory for creating gRPC client connections
// to sibling services within the Model Plane.
package grpcclient

import (
	"context"
	"fmt"
	"log/slog"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
)

// withForwardedMetadata copies the inbound gRPC metadata (notably the caller's
// verified `authorization` bearer) onto the outbound context. orchestrator-core
// proxies these RPCs to session-core, whose interceptor requires that
// credential; gRPC-Go does NOT propagate incoming metadata to outgoing calls
// automatically, so without this the upstream rejects every proxied call with
// "verified caller credential required" (breaking run-events streaming and
// approval decisions through the gateway).
func withForwardedMetadata(ctx context.Context) context.Context {
	if md, ok := metadata.FromIncomingContext(ctx); ok {
		return metadata.NewOutgoingContext(ctx, md)
	}
	return ctx
}

func forwardMetadataUnaryInterceptor(
	ctx context.Context,
	method string,
	req, reply any,
	cc *grpc.ClientConn,
	invoker grpc.UnaryInvoker,
	opts ...grpc.CallOption,
) error {
	return invoker(withForwardedMetadata(ctx), method, req, reply, cc, opts...)
}

func forwardMetadataStreamInterceptor(
	ctx context.Context,
	desc *grpc.StreamDesc,
	cc *grpc.ClientConn,
	method string,
	streamer grpc.Streamer,
	opts ...grpc.CallOption,
) (grpc.ClientStream, error) {
	return streamer(withForwardedMetadata(ctx), desc, cc, method, opts...)
}

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
}

// Dial creates gRPC connections to all configured sibling services.
// Connections that fail to dial are logged as warnings but do not block startup,
// allowing the orchestrator to start even when some services are unavailable.
func Dial(ctx context.Context, opts Options) (*Clients, error) {
	if opts.DialTimeout == 0 {
		opts.DialTimeout = 5 * time.Second
	}

	dialOpts := []grpc.DialOption{
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		// Proxy the caller's verified credential to upstream session-core.
		grpc.WithChainUnaryInterceptor(forwardMetadataUnaryInterceptor),
		grpc.WithChainStreamInterceptor(forwardMetadataStreamInterceptor),
	}

	clients := &Clients{}

	dial := func(addr string, name string) *grpc.ClientConn {
		dialCtx, cancel := context.WithTimeout(ctx, opts.DialTimeout)
		defer cancel()

		conn, err := grpc.DialContext(dialCtx, addr, dialOpts...)
		if err != nil {
			slog.Warn("failed to dial service", "service", name, "addr", addr, "error", err)
			return nil
		}
		slog.Info("connected to service", "service", name, "addr", addr)
		return conn
	}

	clients.SessionCore = dial(opts.SessionCoreAddr, "session-core")
	clients.InferenceCore = dial(opts.InferenceCoreAddr, "inference-core")
	clients.ExecutionCore = dial(opts.ExecutionCoreAddr, "execution-core")
	clients.CapabilityCore = dial(opts.CapabilityCoreAddr, "capability-core")
	clients.SandboxManager = dial(opts.SandboxManagerAddr, "sandbox-manager")
	clients.BrowserBroker = dial(opts.BrowserBrokerAddr, "browser-broker")
	clients.LettaBridge = dial(opts.LettaBridgeAddr, "letta-bridge")

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
