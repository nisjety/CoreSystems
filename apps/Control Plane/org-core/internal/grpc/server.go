package grpc

import (
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
	address    string
	grpcServer *grpcpkg.Server
}

func NewServer(port int) *Server {
	grpcServer := grpcpkg.NewServer()
	healthServer := health.NewServer()
	healthServer.SetServingStatus("", healthpb.HealthCheckResponse_SERVING)
	healthpb.RegisterHealthServer(grpcServer, healthServer)
	reflection.Register(grpcServer)

	return &Server{
		address:    fmt.Sprintf(":%d", port),
		grpcServer: grpcServer,
	}
}

func (s *Server) Start() error {
	listener, err := net.Listen("tcp", s.address)
	if err != nil {
		return fmt.Errorf("listen on %s: %w", s.address, err)
	}

	log.Printf("org-core gRPC listening on %s", s.address)
	if err := s.grpcServer.Serve(listener); err != nil && !errors.Is(err, grpcpkg.ErrServerStopped) {
		return fmt.Errorf("serve gRPC on %s: %w", s.address, err)
	}

	return nil
}

func (s *Server) Stop() {
	if s.grpcServer != nil {
		s.grpcServer.GracefulStop()
	}
}
