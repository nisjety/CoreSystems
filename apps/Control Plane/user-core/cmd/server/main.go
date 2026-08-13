package main

import (
	"context"
	"log"
	"net/http"
	_ "net/http/pprof" // registers /debug/pprof handlers on the default ServeMux
	"os"
	"os/signal"
	"runtime"
	"strconv"
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
	metricsserver "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/metrics"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/nats"
	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/redis"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	"github.com/jackc/pgx/v5/pgxpool"
)

func main() {
	log.Println("Starting Aquatiq User Service...")

	if r := internalkey.AssertFromEnv("USER_CORE_MEMBERSHIP_SERVICE_TOKEN"); !r.OK {
		msg := "[user-core startup] canonical membership credential validation failed (" + string(r.Problem.Kind) + " on " + r.Problem.EnvVar + "): " + r.Problem.Detail
		if internalkey.IsProduction() {
			log.Fatalf("FATAL %s", msg)
		}
		log.Printf("WARN  %s — continuing because not production", msg)
	} else {
		log.Printf("[user-core startup] canonical membership credential OK (%s)", r.Resolved)
	}
	if err := httpserver.ValidateRequiredServiceCredentialRegistry(os.Getenv("USER_CORE_SERVICE_CREDENTIALS")); err != nil {
		msg := "[user-core startup] service credential registry validation failed: " + err.Error()
		if internalkey.IsProduction() {
			log.Fatalf("FATAL %s", msg)
		}
		log.Printf("WARN  %s — continuing because not production", msg)
	} else {
		log.Printf("[user-core startup] required gateway service principal OK")
	}
	if err := grpc.ValidateGRPCServiceCredentialRegistry(); err != nil {
		log.Fatalf("FATAL [user-core startup] gRPC service credential registry validation failed: %v", err)
	}
	authInternalCredential, err := clients.LoadAuthInternalClientCredential()
	if err != nil {
		log.Fatalf("FATAL [user-core startup] Auth internal client credential validation failed: %v", err)
	}
	log.Printf("[user-core startup] scoped Auth and gRPC service principals OK")

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
	natsAuthClient, err := clients.NewNatsAuthClient(
		natsURL,
		authInternalCredential,
	)
	if err != nil {
		log.Fatalf("❌ Failed to create NATS auth client: %v", err)
	}
	defer natsAuthClient.Close()

	// Authenticate via NATS to get service secret (with retries)
	const maxAuthAttempts = 10
	serviceCredential := authInternalCredential
	for attempt := 1; attempt <= maxAuthAttempts; attempt++ {
		log.Printf("🔑 Authenticating user service with auth service via NATS... (attempt %d/%d)", attempt, maxAuthAttempts)
		if err := natsAuthClient.Authenticate(ctx); err != nil {
			if attempt == maxAuthAttempts {
				if internalkey.IsProduction() {
					log.Fatalf("FATAL scoped Auth service-principal verification failed after %d attempts: %v", attempt, err)
				}
				log.Printf("⚠️  Warning: Failed to authenticate user service after %d attempts: %v", attempt, err)
				log.Println("ℹ️  Continuing without NATS authentication in development mode...")
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
			serviceCredential, err = natsAuthClient.GetServiceCredential()
			if err != nil {
				log.Printf("⚠️  Warning: Failed to get verified service credential: %v", err)
				serviceCredential = authInternalCredential
			}
			break
		}
	}

	// Initialize Better Auth client with its scoped service principal.
	betterAuthClient := clients.NewBetterAuthClient(
		betterAuthURL,
		"", // No API key needed, using service secret
	)

	betterAuthClient.SetServicePrincipal(serviceCredential)
	log.Println("✅ Better Auth client configured with scoped service principal")

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

	// Shared verevon-nats should not depend on local controlplane-nats health.
	if sp, spErr := nats.NewSharedPublisher(cfg.NATS.SharedURL, nats.SharedCredentials{
		User: cfg.NATS.SharedUser, Password: cfg.NATS.SharedPass,
		Token: cfg.NATS.SharedToken, AllowTokenFallback: cfg.NATS.SharedAllowTokenFallback,
	}, "user-core"); spErr != nil {
		log.Fatalf("shared NATS unavailable: %v", spErr)
	} else if sp != nil {
		sharedPublisher = sp
		defer sp.Close()
		log.Println("✅ Connected to shared NATS (verevon-nats)")
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

		// G41 (Slice D / §8.30): construct the optional Graph-enrichment
		// dependencies. `NewAuthCoreOAuthClient` returns nil when env is
		// missing — the handler is nil-safe and gracefully falls back to
		// auth-core's ProfileHints (sometimes empty) without enrichment.
		// Live config: `AUTH_SERVICE_URL` already used by `fetchAuthCoreTokenByRef`
		// in the HTTP path; `MICROSOFT_GRAPH_BASE_URL` defaults to the
		// global Graph endpoint when unset.
		authCoreOAuth := clients.NewAuthCoreOAuthClient(
			os.Getenv("AUTH_SERVICE_URL"),
			serviceCredential,
		)
		if authCoreOAuth == nil {
			authCoreOAuth = clients.NewAuthCoreOAuthClient(
				os.Getenv("BETTER_AUTH_URL"),
				serviceCredential,
			)
		}
		if authCoreOAuth == nil {
			log.Println("ℹ️  Graph enrichment disabled: auth-core OAuth client URL not configured")
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
	defer userService.CloseAuditOutbox()
	defer userService.CloseErasureSaga()
	go func() {
		purge := func() {
			purgeCtx, purgeCancel := context.WithTimeout(ctx, 30*time.Second)
			defer purgeCancel()
			count, purgeErr := userService.PurgeExpiredOnboardingDrafts(purgeCtx)
			if purgeErr != nil {
				log.Printf("onboarding draft retention sweep failed: %v", purgeErr)
				return
			}
			if count > 0 {
				log.Printf("cleared %d expired onboarding drafts", count)
			}
		}

		purge()
		ticker := time.NewTicker(time.Hour)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				purge()
			}
		}
	}()

	// Wire shared cross-plane publisher to userService for domain events
	if sharedPublisher != nil {
		userService.SetSharedPublisher(sharedPublisher)
		log.Println("✅ user-core service connected to shared NATS for event publishing")
	}

	// Wire the local control-plane bus (controlplane-nats) as the audit
	// publisher so verevon.audit.v1.control.* (erasure/DSAR) events reach
	// audit-core's primary subscription. Kept separate from the shared
	// verevon-nats bus, which carries cross-plane domain/ACL/fan-out events.
	if natsClient != nil {
		userService.SetAuditPublisher(natsClient)
		log.Println("✅ user-core audit events wired to local control-plane bus (controlplane-nats)")
	}

	// Wire the secondary auth_service DB pool used by GDPR erasure to invoke
	// gdpr_hard_delete_user / gdpr_anonymize_user (those procs live in the
	// auth_service DB, not user-core's user_service DB). When AUTH_DATABASE_URL
	// is unset the erasure endpoints return a clear "not configured" error
	// rather than silently skipping auth-side data.
	if authDSN := strings.TrimSpace(os.Getenv("AUTH_DATABASE_URL")); authDSN != "" {
		authPool, apErr := pgxpool.New(ctx, authDSN)
		if apErr != nil {
			log.Printf("⚠️  GDPR: failed to open auth_service pool (AUTH_DATABASE_URL): %v", apErr)
		} else if pingErr := authPool.Ping(ctx); pingErr != nil {
			log.Printf("⚠️  GDPR: auth_service pool ping failed: %v", pingErr)
			authPool.Close()
		} else {
			defer authPool.Close()
			userService.SetAuthPool(authPool)
			log.Println("✅ GDPR: connected to auth_service DB for gdpr_hard_delete_user / gdpr_anonymize_user")
		}
	} else {
		log.Println("⚠️  GDPR: AUTH_DATABASE_URL not set — hard-erase/anonymize routes will return 503 erasure_unavailable (never an opaque 500); DSAR export still works")
	}

	// Wire the org-core succession client used by the admin-succession
	// pre-flight (EnsureSuccession, internal/users/gdpr_succession.go): before
	// a sole owner/admin's erasure saga starts, user-core must hand their seat
	// off to a validated successor in org-core. When any of
	// ORG_CORE_BASE_URL / ORG_CORE_SERVICE_PRINCIPAL / ORG_CORE_SERVICE_TOKEN
	// is unset, NewOrgCoreClient returns nil and EnsureSuccession fails closed
	// (503) ONLY for the users who actually need a successor — everyone else's
	// erasure is unaffected.
	orgCoreServicePrincipal := strings.TrimSpace(os.Getenv("ORG_CORE_SERVICE_PRINCIPAL"))
	if orgCoreServicePrincipal == "" {
		orgCoreServicePrincipal = "user-core"
	}
	if orgCoreClient := clients.NewOrgCoreClient(
		os.Getenv("ORG_CORE_BASE_URL"),
		orgCoreServicePrincipal,
		os.Getenv("ORG_CORE_SERVICE_TOKEN"),
	); orgCoreClient != nil {
		userService.SetOrgCoreClient(orgCoreClient)
		log.Println("✅ GDPR: org-core succession client wired (admin-succession pre-flight active)")
	} else {
		log.Println("⚠️  GDPR: ORG_CORE_BASE_URL/ORG_CORE_SERVICE_TOKEN not set — sole-owner/admin self-erasure will return 503 succession_unavailable until a successor client is configured")
	}
	userService.StartErasureSaga()

	// Create gRPC server with NATS publisher and Better Auth client
	grpcServer := grpc.NewServer(cfg, db, natsPublisher, sharedPublisher, betterAuthClient)

	// Create HTTP/REST server (Phase 4: Frontend compatibility)
	httpPort := os.Getenv("HTTP_PORT")
	if httpPort == "" {
		httpPort = "3012"
	}
	log.Printf("🌐 Initializing HTTP/REST server on port %s (Phase 4: Frontend parity)", httpPort)
	// Per-user authz facade (ListVisible/Check) is served over HTTP so Data
	// Plane services (documents-api, retrieval) can resolve grants cross-plane.
	aclRepo := users.NewAclRepository(db)
	httpServer := httpserver.NewServer(userService, aclRepo, sharedPublisher, redisClient, httpPort)
	httpServer.SetSpaceRepository(spaces.NewRepository(db))
	httpServer.SetAuthInternalCredential(serviceCredential)

	// Prometheus /metrics on a dedicated port (default 9091), scraped by the
	// Control-Plane Prometheus (Phase 6 B13).
	metricsPort := 9091
	if v := strings.TrimSpace(os.Getenv("METRICS_PORT")); v != "" {
		if n, convErr := strconv.Atoi(v); convErr == nil && n > 0 {
			metricsPort = n
		}
	}
	metricsServer := metricsserver.NewServer(metricsPort)

	// Handle shutdown gracefully
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, os.Interrupt, syscall.SIGTERM)

	// Start servers in goroutines
	errChan := make(chan error, 3)

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

	// Start Prometheus metrics server
	go func() {
		if err := metricsServer.Start(); err != nil {
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
	if err := metricsServer.Shutdown(shutdownCtx); err != nil {
		log.Printf("metrics server shutdown error: %v", err)
	}

	log.Println("Shutdown complete")
}
