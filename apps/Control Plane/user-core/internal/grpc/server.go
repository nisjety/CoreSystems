package grpc

import (
	"context"
	"crypto/subtle"
	"fmt"
	"log"
	"net"
	"os"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/clients"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/handlers"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/nats"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	pb "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/proto/user/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/reflection"
	"google.golang.org/grpc/status"
)

const internalAPIKeyMetadataKey = "x-internal-api-key"

// Server represents the gRPC server
type Server struct {
	config           *config.Config
	db               *database.DB
	userService      *users.Service
	publisher        *nats.Publisher
	sharedPublisher  *nats.SharedPublisher
	betterAuthClient *clients.BetterAuthClient
	userRepo         *users.Repository
	grpcServer       *grpc.Server
	internalKeys     []string
}

// NewServer creates a new gRPC server instance. sharedPublisher (may be nil)
// emits cross-plane grant-change events so the Data Plane retrieval cache evicts
// on revoke — closing the previously nil-wired DocumentAclHandler publisher.
func NewServer(cfg *config.Config, db *database.DB, publisher *nats.Publisher, sharedPublisher *nats.SharedPublisher, betterAuthClient *clients.BetterAuthClient) *Server {
	// Create repositories
	userRepo := users.NewRepository(db)

	// Create services - Phase 4: Updated to support both gRPC and HTTP
	userService := users.NewService(userRepo, betterAuthClient, publisher)

	return &Server{
		config:           cfg,
		db:               db,
		userService:      userService,
		publisher:        publisher,
		sharedPublisher:  sharedPublisher,
		betterAuthClient: betterAuthClient,
		userRepo:         userRepo,
		internalKeys:     configuredInternalKeys(),
	}
}

// Start starts the gRPC server
func (s *Server) Start(ctx context.Context) error {
	address := fmt.Sprintf(":%d", s.config.Server.GRPCPort)

	listener, err := net.Listen("tcp", address)
	if err != nil {
		return fmt.Errorf("failed to listen on %s: %w", address, err)
	}

	// Create gRPC server with options
	s.grpcServer = grpc.NewServer(
		grpc.ChainUnaryInterceptor(
			s.requireInternalKeyUnaryInterceptor,
			s.loggingInterceptor,
		),
		grpc.ChainStreamInterceptor(
			s.requireInternalKeyStreamInterceptor,
		),
	)

	// Register services
	pb.RegisterUserServiceServer(s.grpcServer, &userServiceHandler{
		userService:      s.userService,
		publisher:        s.publisher,
		betterAuthClient: s.betterAuthClient,
		repo:             s.userRepo,
	})

	aclRepo := users.NewAclRepository(s.db)
	dAclHandler := handlers.NewDocumentAclHandler(aclRepo, s.publisher, s.sharedPublisher)
	pb.RegisterDocumentAccessServiceServer(s.grpcServer, dAclHandler)

	// Enable reflection for grpcurl
	reflection.Register(s.grpcServer)

	log.Printf("🚀 User Service gRPC server listening on %s", address)

	// Start server in a goroutine
	go func() {
		if err := s.grpcServer.Serve(listener); err != nil {
			log.Printf("Failed to serve: %v", err)
		}
	}()

	// Wait for context cancellation
	<-ctx.Done()

	// Graceful shutdown
	log.Println("Shutting down gRPC server...")
	s.grpcServer.GracefulStop()
	log.Println("gRPC server stopped")

	return nil
}

func configuredInternalKeys() []string {
	values := make([]string, 0, 2)
	seen := map[string]struct{}{}

	for _, name := range []string{"INTERNAL_API_KEY", "INTERNAL_SERVICE_SECRET"} {
		value := strings.TrimSpace(os.Getenv(name))
		if value == "" {
			continue
		}
		if _, ok := seen[value]; ok {
			continue
		}
		seen[value] = struct{}{}
		values = append(values, value)
	}

	return values
}

func (s *Server) requireInternalKeyUnaryInterceptor(
	ctx context.Context,
	req interface{},
	info *grpc.UnaryServerInfo,
	handler grpc.UnaryHandler,
) (interface{}, error) {
	if err := s.validateInternalGRPCKey(ctx); err != nil {
		log.Printf("gRPC auth denied: %s - %v", info.FullMethod, err)
		return nil, err
	}
	return handler(ctx, req)
}

func (s *Server) requireInternalKeyStreamInterceptor(
	srv interface{},
	stream grpc.ServerStream,
	info *grpc.StreamServerInfo,
	handler grpc.StreamHandler,
) error {
	if err := s.validateInternalGRPCKey(stream.Context()); err != nil {
		log.Printf("gRPC stream auth denied: %s - %v", info.FullMethod, err)
		return err
	}
	return handler(srv, stream)
}

func (s *Server) validateInternalGRPCKey(ctx context.Context) error {
	if len(s.internalKeys) == 0 {
		return status.Error(codes.FailedPrecondition, "service auth not configured")
	}

	md, ok := metadata.FromIncomingContext(ctx)
	if !ok {
		return status.Error(codes.Unauthenticated, "internal API key is required")
	}

	for _, candidate := range md.Get(internalAPIKeyMetadataKey) {
		for _, configured := range s.internalKeys {
			if constantTimeEqual(candidate, configured) {
				return nil
			}
		}
	}

	return status.Error(codes.Unauthenticated, "invalid internal API key")
}

func constantTimeEqual(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1
}

// Stop stops the gRPC server
func (s *Server) Stop() {
	if s.grpcServer != nil {
		s.grpcServer.GracefulStop()
	}
}

// loggingInterceptor logs gRPC requests
func (s *Server) loggingInterceptor(
	ctx context.Context,
	req interface{},
	info *grpc.UnaryServerInfo,
	handler grpc.UnaryHandler,
) (interface{}, error) {
	log.Printf("gRPC Request: %s", info.FullMethod)

	resp, err := handler(ctx, req)

	if err != nil {
		log.Printf("gRPC Error: %s - %v", info.FullMethod, err)
	} else {
		log.Printf("gRPC Success: %s", info.FullMethod)
	}

	return resp, err
}

// UserService returns the user service (for sharing between servers)
func (s *Server) UserService() *users.Service {
	return s.userService
}

// Database returns the database connection (for sharing between servers)
func (s *Server) Database() *database.DB {
	return s.db
}
