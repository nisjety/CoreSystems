package main

import (
	"context"
	"log"
	"net/http"
	_ "net/http/pprof" // registers /debug/pprof handlers on the default ServeMux
	"os"
	"os/signal"
	"runtime"
	"strings"
	"syscall"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/clients"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/grpc"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/handlers"
	httpserver "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/http"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/internalkey"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/nats"
	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/redis"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

func main() {
	log.Println("Starting Aquatiq User Service...")

	// G40 (velion-gap.md §8.29): refuse to start in production with a
	// placeholder / missing / drifted internal API key. Mirrors velion's
	// boot-time gate (§8.27).
	if r := internalkey.AssertFromEnv("INTERNAL_API_KEY", "INTERNAL_SERVICE_SECRET"); !r.OK {
		msg := "[user-core startup] internal API key validation failed (" + string(r.Problem.Kind) + " on " + r.Problem.EnvVar + "): " + r.Problem.Detail
		if internalkey.IsProduction() {
			log.Fatalf("FATAL %s", msg)
		}
		log.Printf("WARN  %s — continuing because not production", msg)
	} else {
		log.Printf("[user-core startup] internal API key OK (%s)", r.Resolved)
	}

	// pprof debug server — enable with PPROF_ENABLED=true; default addr :6060
	if os.Getenv("PPROF_ENABLED") == "true" {
		pprofAddr := os.Getenv("PPROF_ADDR")
		if pprofAddr == "" {
			pprofAddr = ":6060"
		}
		// Enable mutex and block profiling at low rates for production safety
		runtime.SetMutexProfileFraction(10)
		runtime.SetBlockProfileRate(100_000_000) // 100ms
		go func() {
			log.Printf("pprof debug server listening on %s", pprofAddr)
			if err := http.ListenAndServe(pprofAddr, nil); err != nil {
				log.Printf("pprof server error: %v", err)
			}
		}()
	}

	// Load configuration
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("Failed to load config: %v", err)
	}

	log.Printf("Environment: %s", cfg.Server.Environment)

	// Create context that listens for shutdown signals
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	// Connect to database
	log.Println("Connecting to database...")
	db, err := database.Connect(ctx, &cfg.Database)
	if err != nil {
		log.Fatalf("Failed to connect to database: %v", err)
	}
	defer db.Close()

	log.Println("Database connected successfully")

	// Run database migrations
	log.Println("Running database migrations...")
	migrationsDir := os.Getenv("MIGRATIONS_DIR")
	if migrationsDir == "" {
		migrationsDir = "./migrations"
	}
	if err := db.RunMigrations(ctx, migrationsDir); err != nil {
		log.Fatalf("Failed to run migrations: %v", err)
	}
	log.Println("Migrations complete")
	log.Println("🔌 Initializing Better Auth client...")

	// Get NATS URL and Better Auth URL from env
	natsURL := os.Getenv("NATS_URL")
	if natsURL == "" {
		natsURL = "nats://localhost:4222"
	}

	betterAuthURL := os.Getenv("BETTER_AUTH_URL")
	if betterAuthURL == "" {
		betterAuthURL = "http://auth-service:3000"
	}

	// Initialize NATS authentication client
	log.Println("🔌 Initializing NATS authentication...")
	serviceAuthSecret := strings.TrimSpace(os.Getenv("INTERNAL_SERVICE_SECRET"))
	if serviceAuthSecret == "" {
		serviceAuthSecret = strings.TrimSpace(os.Getenv("INTERNAL_API_KEY"))
	}
	if serviceAuthSecret == "" {
		log.Fatal("INTERNAL_SERVICE_SECRET or INTERNAL_API_KEY must be set")
	}

	natsAuthClient, err := clients.NewNatsAuthClient(
		natsURL,
		"user-service",
		serviceAuthSecret,
	)
	if err != nil {
		log.Fatalf("❌ Failed to create NATS auth client: %v", err)
	}
	defer natsAuthClient.Close()

	// Authenticate via NATS to get service secret (with retries)
	const maxAuthAttempts = 10
	var serviceSecret string
	for attempt := 1; attempt <= maxAuthAttempts; attempt++ {
		log.Printf("🔑 Authenticating user service with auth service via NATS... (attempt %d/%d)", attempt, maxAuthAttempts)
		if err := natsAuthClient.Authenticate(ctx); err != nil {
			if attempt == maxAuthAttempts {
				// In development, continue without auth instead of crashing
				log.Printf("⚠️  Warning: Failed to authenticate user service after %d attempts: %v", attempt, err)
				log.Println("ℹ️  Continuing without NATS authentication in development mode...")
				serviceSecret = serviceAuthSecret // Fallback to resolved secret
				break
			}
			wait := time.Duration(attempt*2) * time.Second
			log.Printf("⚠️ Authentication failed: %v. Retrying in %s...", err, wait)
			select {
			case <-time.After(wait):
				continue
			case <-ctx.Done():
				log.Fatalf("❌ Authentication cancelled: %v", ctx.Err())
			}
		} else {
			log.Println("✅ User service authenticated successfully via NATS")
			var err error
			serviceSecret, err = natsAuthClient.GetServiceSecret()
			if err != nil {
				log.Printf("⚠️  Warning: Failed to get service secret: %v", err)
				serviceSecret = serviceAuthSecret // Fallback
			}
			break
		}
	}

	// Initialize Better Auth client with service secret
	betterAuthClient := clients.NewBetterAuthClient(
		betterAuthURL,
		"", // No API key needed, using service secret
	)

	// Use service secret (either from NATS or fallback)
	if serviceSecret != "" {
		betterAuthClient.SetServiceSecret(serviceSecret)
		log.Println("✅ Better Auth client configured with service secret")
	} else {
		log.Println("⚠️  Better Auth client running without service secret (dev mode)")
	}

	// Initialize user repository
	userRepo := users.NewRepository(db)

	// Initialize Redis cache (optional — gracefully degraded when disabled)
	var redisClient *rediscache.Client
	if cfg.Redis.Enabled {
		log.Println("🔌 Connecting to Redis...")
		redisClient, err = rediscache.NewClient(rediscache.Config{
			Host:     cfg.Redis.Host,
			Port:     cfg.Redis.Port,
			Password: cfg.Redis.Password,
			DB:       cfg.Redis.DB,
		})
		if err != nil {
			log.Printf("⚠️  Warning: Redis unavailable — running without cache: %v", err)
			redisClient = nil
		} else {
			defer redisClient.Close()
			log.Println("✅ Redis cache connected")
		}
	}

	// Initialize NATS client for events (declare variables first)
	var natsClient *nats.Client
	var natsSubscriber *nats.Subscriber
	var natsPublisher *nats.Publisher
	var sharedPublisher *nats.SharedPublisher

	// Shared velion-nats should not depend on local controlplane-nats health.
	if sp, spErr := nats.NewSharedPublisher(cfg.NATS.SharedURL, cfg.NATS.SharedToken, "user-core"); spErr != nil {
		log.Printf("⚠️  Shared NATS unavailable: %v", spErr)
	} else if sp != nil {
		sharedPublisher = sp
		defer sp.Close()
		log.Println("✅ Connected to shared NATS (velion-nats)")
	}

	log.Printf("Connecting to NATS: %s", natsURL)
	natsToken := os.Getenv("NATS_TOKEN")
	if natsToken == "" {
		natsToken = os.Getenv("NATS_AUTH_TOKEN") // Fallback
	}
	natsClient, err = nats.NewClient(nats.Config{
		URL:                  natsURL,
		MaxReconnectAttempts: -1, // Infinite reconnection
		ReconnectWait:        2 * time.Second,
		Name:                 "user-service-go",
		Token:                natsToken,
	})
	if err != nil {
		log.Printf("⚠️  Warning: Failed to connect to NATS: %v", err)
		log.Println("ℹ️  Continuing without NATS event streaming...")
	} else {
		defer natsClient.Close()

		// Initialize publisher
		natsPublisher = nats.NewPublisher(natsClient)

		// Ensure USER_EVENTS stream exists
		if err := natsPublisher.EnsureUserEventsStream(ctx); err != nil {
			log.Printf("⚠️  Warning: Failed to ensure USER_EVENTS stream: %v", err)
		}

		// G41 (Slice D / §8.30): construct the optional Graph-enrichment
		// dependencies. `NewAuthCoreOAuthClient` returns nil when env is
		// missing — the handler is nil-safe and gracefully falls back to
		// auth-core's ProfileHints (sometimes empty) without enrichment.
		// Live config: `AUTH_SERVICE_URL` already used by `fetchAuthCoreTokenByRef`
		// in the HTTP path; `MICROSOFT_GRAPH_BASE_URL` defaults to the
		// global Graph endpoint when unset.
		authCoreOAuth := clients.NewAuthCoreOAuthClient(
			os.Getenv("AUTH_SERVICE_URL"),
			os.Getenv("INTERNAL_API_KEY"),
		)
		if authCoreOAuth == nil {
			// Fall back to BETTER_AUTH_URL / INTERNAL_SERVICE_SECRET as the
			// HTTP handler does (mirrors fetchAuthCoreTokenByRef precedence).
			authCoreOAuth = clients.NewAuthCoreOAuthClient(
				os.Getenv("BETTER_AUTH_URL"),
				os.Getenv("INTERNAL_SERVICE_SECRET"),
			)
		}
		if authCoreOAuth == nil {
			log.Println("ℹ️  Graph enrichment disabled: auth-core OAuth client not configured (AUTH_SERVICE_URL + INTERNAL_API_KEY)")
		} else {
			log.Println("✅ Graph enrichment: auth-core OAuth client ready")
		}
		graphClient := clients.NewMicrosoftGraphClient(os.Getenv("MICROSOFT_GRAPH_BASE_URL"))

		// Initialize event handler
		eventHandler := handlers.NewEventHandler(userRepo, natsPublisher, sharedPublisher, authCoreOAuth, graphClient)

		// Initialize subscriber
		natsSubscriber = nats.NewSubscriber(natsClient, eventHandler)

		// Start listening to auth events
		if err := natsSubscriber.Start(ctx); err != nil {
			log.Printf("⚠️  Warning: Failed to start NATS subscriber: %v", err)
		} else {
			defer natsSubscriber.Stop()
		}
	}

	// Initialize user service (Phase 4: After NATS setup so publisher is available)
	userService := users.NewService(userRepo, betterAuthClient, natsPublisher, redisClient)

	// Wire shared cross-plane publisher to userService for domain events
	if sharedPublisher != nil {
		userService.SetSharedPublisher(sharedPublisher)
		log.Println("✅ user-core service connected to shared NATS for event publishing")
	}

	// Create gRPC server with NATS publisher and Better Auth client
	grpcServer := grpc.NewServer(cfg, db, natsPublisher, betterAuthClient)

	// Create HTTP/REST server (Phase 4: Frontend compatibility)
	httpPort := os.Getenv("HTTP_PORT")
	if httpPort == "" {
		httpPort = "3012"
	}
	log.Printf("🌐 Initializing HTTP/REST server on port %s (Phase 4: Frontend parity)", httpPort)
	httpServer := httpserver.NewServer(userService, httpPort)

	// Handle shutdown gracefully
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, os.Interrupt, syscall.SIGTERM)

	// Start servers in goroutines
	errChan := make(chan error, 2)

	// Start gRPC server
	go func() {
		if err := grpcServer.Start(ctx); err != nil {
			errChan <- err
		}
	}()

	// Start HTTP server
	go func() {
		log.Println("✅ HTTP/REST server starting...")
		if err := httpServer.Start(); err != nil {
			errChan <- err
		}
	}()

	// Wait for shutdown signal or error
	select {
	case <-sigChan:
		log.Println("Received shutdown signal")
		cancel()
	case err := <-errChan:
		log.Printf("Server error: %v", err)
		cancel()
	}

	// Graceful shutdown with timeout
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()

	log.Println("Shutting down HTTP server...")
	if err := httpServer.Shutdown(shutdownCtx); err != nil {
		log.Printf("HTTP server shutdown error: %v", err)
	}

	log.Println("Shutdown complete")
}
