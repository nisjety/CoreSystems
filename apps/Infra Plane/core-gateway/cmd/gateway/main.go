package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/coresystem/integration-gateway/internal/audit"
	"github.com/coresystem/integration-gateway/internal/cache"
	"github.com/coresystem/integration-gateway/internal/config"
	"github.com/coresystem/integration-gateway/internal/docker"
	"github.com/coresystem/integration-gateway/internal/grpc"
	"github.com/coresystem/integration-gateway/internal/health"
	"github.com/coresystem/integration-gateway/internal/ratelimit"
	"github.com/coresystem/integration-gateway/internal/whitelist"
	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"

	databasev1 "github.com/coresystem/integration-gateway/api/proto/database/v1"
	dockerv1 "github.com/coresystem/integration-gateway/api/proto/docker/v1"
	healthv1 "github.com/coresystem/integration-gateway/api/proto/health/v1"
	whitelistv1 "github.com/coresystem/integration-gateway/api/proto/whitelist/v1"
	grpcServer "google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/reflection"
)

func main() {
	fmt.Println("🚀 Aquatiq Integration Gateway - Starting...")

	// Initialize audit logger
	auditLogger, err := audit.NewAuditLogger("info")
	if err != nil {
		fmt.Printf("❌ Failed to create audit logger: %v\n", err)
		os.Exit(1)
	}
	defer auditLogger.Sync()

	fmt.Println("✅ Audit logger initialized")

	// Load configuration
	cfg, err := config.Load()
	if err != nil {
		fmt.Printf("❌ Failed to load configuration: %v\n", err)
		// For testing, use defaults if config load fails
		cfg = getDefaultConfig()
		fmt.Println("⚠️  Using default configuration")
	} else {
		fmt.Println("✅ Configuration loaded")
	}

	// Initialize Redis cache (optional - graceful degradation)
	var redisCache *cache.RedisCache
	if cfg.Redis.Enabled {
		redisCache, err = cache.NewRedisCache(cfg.Redis)
		if err != nil {
			fmt.Printf("⚠️  Failed to connect to Redis (will use local cache): %v\n", err)
			redisCache = nil
		} else {
			fmt.Println("✅ Redis cache connected")
			defer redisCache.Close()
		}
	} else {
		fmt.Println("ℹ️  Redis cache disabled in configuration")
	}

	// Initialize rate limiter
	rateLimiter := ratelimit.New(ratelimit.Config{
		GlobalRPS:   cfg.RateLimit.GlobalRPS,
		AdminRPS:    cfg.RateLimit.AdminRPS,
		BurstSize:   cfg.RateLimit.BurstSize,
		Distributed: cfg.RateLimit.Distributed && redisCache != nil,
		Cache:       redisCache,
		AuditLogger: auditLogger,
	})
	fmt.Println("✅ Rate limiter initialized")

	// Create circuit breakers for each integration
	// Initialize managers for gRPC services

	// Docker manager
	dockerManager, err := docker.NewManager(cfg.Docker, auditLogger)
	if err != nil {
		fmt.Printf("⚠️  Failed to initialize Docker manager: %v\n", err)
		dockerManager = nil
	} else {
		fmt.Println("✅ Docker manager initialized")
		defer dockerManager.Close()
	}

	// Whitelist manager
	whitelistManager, err := whitelist.NewManager(whitelist.Config{
		TraefikConfigPath: cfg.Whitelist.TraefikConfigPath,
		AuditLogger:       auditLogger,
	})
	if err != nil {
		fmt.Printf("⚠️  Failed to initialize whitelist manager: %v\n", err)
		whitelistManager = nil
	} else {
		fmt.Println("✅ Whitelist manager initialized")
	}

	// Database health checker
	dbChecker := health.NewDatabaseChecker(health.Config{
		PostgresURL: cfg.Database.PostgresURL,
		RedisCache:  redisCache,
	})
	fmt.Println("✅ Database health checker initialized")

	// Health checker (comprehensive)
	// NewHealthChecker(dbChecker *DatabaseChecker, version string)
	healthChecker := health.NewHealthChecker(dbChecker, "1.0.0")
	fmt.Println("✅ Health checker initialized")

	fmt.Println("\n✅ All infrastructure components initialized!")

	// Setup Chi router
	r := chi.NewRouter()

	// Global middleware
	r.Use(middleware.RequestID)
	r.Use(middleware.RealIP)
	r.Use(middleware.Logger)
	r.Use(middleware.Recoverer)
	r.Use(middleware.Compress(5)) // Add gzip compression (level 5 = good balance)
	r.Use(middleware.Timeout(60 * time.Second))

	// Apply rate limiting to all routes
	r.Use(rateLimiter.Middleware("global"))

	// Public endpoints
	r.Get("/health", func(w http.ResponseWriter, r *http.Request) {
		health := map[string]interface{}{
			"status":    "healthy",
			"timestamp": time.Now().Format(time.RFC3339),
		}

		// Check Redis connection
		if redisCache != nil {
			if err := redisCache.Ping(); err != nil {
				health["redis"] = map[string]string{
					"status": "disconnected",
					"error":  err.Error(),
				}
			} else {
				stats := redisCache.PoolStats()
				health["redis"] = map[string]interface{}{
					"status":      "connected",
					"connections": stats.TotalConns,
					"idle":        stats.IdleConns,
					"hits":        stats.Hits,
					"misses":      stats.Misses,
				}
			}
		} else {
			health["redis"] = "not_configured"
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(health)
	})

	// Admin endpoints (with stricter rate limiting)
	r.Group(func(r chi.Router) {
		r.Use(rateLimiter.Middleware("admin"))

		// Rate limiter stats
		r.Get("/rate-limiter", func(w http.ResponseWriter, r *http.Request) {
			stats := rateLimiter.GetStats()
			w.Header().Set("Content-Type", "application/json")
			json.NewEncoder(w).Encode(stats)
		})

		// Cache stats
		r.Get("/cache/stats", func(w http.ResponseWriter, r *http.Request) {
			if redisCache == nil {
				w.WriteHeader(http.StatusServiceUnavailable)
				json.NewEncoder(w).Encode(map[string]string{
					"error": "Redis cache not configured",
				})
				return
			}

			stats := redisCache.PoolStats()
			w.Header().Set("Content-Type", "application/json")
			json.NewEncoder(w).Encode(map[string]interface{}{
				"total_conns": stats.TotalConns,
				"idle_conns":  stats.IdleConns,
				"stale_conns": stats.StaleConns,
				"hits":        stats.Hits,
				"misses":      stats.Misses,
				"timeouts":    stats.Timeouts,
			})
		})
	})

	// Start HTTP REST server
	restAddr := fmt.Sprintf("%s:%d", cfg.Server.Host, cfg.Server.Port)
	restSrv := &http.Server{
		Addr:         restAddr,
		Handler:      r,
		ReadTimeout:  cfg.Server.ReadTimeout,
		WriteTimeout: cfg.Server.WriteTimeout,
	}

	// Start REST server in goroutine
	go func() {
		fmt.Printf("\n🌐 REST API listening on http://%s\n", restAddr)
		fmt.Println("📍 REST Endpoints:")
		fmt.Println("  - GET  /health              - Health check")
		fmt.Println("  - GET  /rate-limiter        - Rate limiter stats (admin)")
		fmt.Println("  - GET  /cache/stats         - Redis cache stats (admin)")

		if err := restSrv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			fmt.Printf("❌ REST server error: %v\n", err)
			os.Exit(1)
		}
	}()

	// Setup gRPC server
	grpcAddr := fmt.Sprintf("%s:%d", cfg.Server.Host, cfg.Server.GRPCPort)
	lis, err := net.Listen("tcp", grpcAddr)
	if err != nil {
		fmt.Printf("❌ Failed to listen on gRPC port: %v\n", err)
		os.Exit(1)
	}

	// Load TLS credentials for gRPC server (if configured)
	var grpcOpts []grpcServer.ServerOption
	grpcOpts = append(grpcOpts,
		grpcServer.MaxRecvMsgSize(10*1024*1024), // 10MB
		grpcServer.MaxSendMsgSize(10*1024*1024), // 10MB
	)

	// Check if TLS is enabled
	if cfg.GRPC.TLS.Enabled {
		creds, err := credentials.NewServerTLSFromFile(
			cfg.GRPC.TLS.CertFile,
			cfg.GRPC.TLS.KeyFile,
		)
		if err != nil {
			fmt.Printf("❌ Failed to load TLS credentials: %v\n", err)
			os.Exit(1)
		}
		grpcOpts = append(grpcOpts, grpcServer.Creds(creds))
		fmt.Println("🔒 gRPC TLS enabled")
	} else {
		fmt.Println("⚠️  gRPC TLS disabled - using plaintext (not recommended for production)")
	}

	// Create gRPC server with options
	grpcSrv := grpcServer.NewServer(grpcOpts...)

	// Register gRPC services
	if healthChecker != nil {
		healthv1.RegisterHealthServiceServer(grpcSrv, grpc.NewHealthServiceServer(healthChecker, dbChecker))
		fmt.Println("✅ Health gRPC service registered")
	}

	if dockerManager != nil {
		dockerv1.RegisterDockerServiceServer(grpcSrv, grpc.NewDockerServiceServer(dockerManager))
		fmt.Println("✅ Docker gRPC service registered")
	}

	if whitelistManager != nil {
		whitelistv1.RegisterWhitelistServiceServer(grpcSrv, grpc.NewWhitelistServiceServer(whitelistManager))
		fmt.Println("✅ Whitelist gRPC service registered")
	}

	if dbChecker != nil {
		databasev1.RegisterDatabaseServiceServer(grpcSrv, grpc.NewDatabaseServiceServer(dbChecker))
		fmt.Println("✅ Database gRPC service registered")
	}

	// Register reflection service (for tools like grpcurl)
	reflection.Register(grpcSrv)
	fmt.Println("✅ gRPC reflection registered")

	// Start gRPC server in goroutine
	go func() {
		fmt.Printf("\n⚡ gRPC API listening on %s\n", grpcAddr)
		fmt.Println("📍 gRPC Services:")
		fmt.Println("  - coresystem.gateway.health.v1.HealthService")
		fmt.Println("  - coresystem.gateway.docker.v1.DockerService")
		fmt.Println("  - coresystem.gateway.whitelist.v1.WhitelistService")
		fmt.Println("  - coresystem.gateway.database.v1.DatabaseService")
		fmt.Println("\n💡 Test with: grpcurl -plaintext localhost:50051 list")
		fmt.Println("\nPress Ctrl+C to shutdown...")

		if err := grpcSrv.Serve(lis); err != nil {
			fmt.Printf("❌ gRPC server error: %v\n", err)
			os.Exit(1)
		}
	}()

	// Wait for interrupt signal
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, os.Interrupt, syscall.SIGTERM)
	<-sigChan

	fmt.Println("\n👋 Shutting down gracefully...")

	// Shutdown REST server with timeout
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), cfg.Server.ShutdownTimeout)
	defer shutdownCancel()

	if err := restSrv.Shutdown(shutdownCtx); err != nil {
		fmt.Printf("⚠️  REST server shutdown error: %v\n", err)
	}

	// Shutdown gRPC server gracefully
	grpcSrv.GracefulStop()
	fmt.Println("✅ gRPC server stopped")

	auditLogger.Sync()
	fmt.Println("✅ Shutdown complete")
}

// getDefaultConfig returns default configuration for testing
func getDefaultConfig() *config.Config {
	return &config.Config{
		Server: config.ServerConfig{
			Host:            "0.0.0.0",
			Port:            7500,
			ReadTimeout:     30 * time.Second,
			WriteTimeout:    30 * time.Second,
			ShutdownTimeout: 30 * time.Second,
			APIKey:          "test-api-key",
		},
	}
}
