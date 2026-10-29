package grpc

import (
	"context"
	"errors"
	"fmt"
	"log"
	"net"

	grpcpkg "google.golang.org/grpc"
	"google.golang.org/grpc/health"
	healthpb "google.golang.org/grpc/health/grpc_health_v1"
	"google.golang.org/grpc/reflection"
)

type Server struct {
	port       int
	grpcServer *grpcpkg.Server
}

func NewServer(port int) *Server {
	grpcServer := grpcpkg.NewServer()
	healthServer := health.NewServer()
	healthServer.SetServingStatus("", healthpb.HealthCheckResponse_SERVING)
	healthpb.RegisterHealthServer(grpcServer, healthServer)
	reflection.Register(grpcServer)

	return &Server{
		port:       port,
		grpcServer: grpcServer,
	}
}

func (s *Server) Start() error {
	listener, err := net.Listen("tcp", fmt.Sprintf(":%d", s.port))
	if err != nil {
		return fmt.Errorf("listen on grpc port %d: %w", s.port, err)
	}

	log.Printf("billing-core gRPC listening on :%d", s.port)
	if err := s.grpcServer.Serve(listener); err != nil && !errors.Is(err, grpcpkg.ErrServerStopped) {
		return fmt.Errorf("serve gRPC on port %d: %w", s.port, err)
	}

	return nil
}

func (s *Server) Shutdown(ctx context.Context) error {
	done := make(chan struct{})
	go func() {
		s.grpcServer.GracefulStop()
		close(done)
	}()

	select {
	case <-done:
		return nil
	case <-ctx.Done():
		s.grpcServer.Stop()
		return ctx.Err()
	}
}
