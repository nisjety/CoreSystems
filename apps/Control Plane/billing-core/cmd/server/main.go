package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"net/http"
	_ "net/http/pprof"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/adapters/hyperswitch"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/adapters/lago"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/adapters/nexi"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/adapters/stripe"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/database"
	grpcserver "github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/grpc"
	httpserver "github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/http"
	metricsserver "github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/metrics"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/nats"
	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/redis"
)

func main() {
	if err := httpserver.ValidateRequiredServiceCredentialRegistry(os.Getenv("BILLING_CORE_SERVICE_CREDENTIALS")); err != nil {
		log.Fatalf("[billing-core startup] service credential registry validation failed: %v", err)
	}
	log.Printf("[billing-core startup] scoped service credential registry OK")

	// pprof debug server — enable with PPROF_ENABLED=true; default addr :6062
	if os.Getenv("PPROF_ENABLED") == "true" {
		pprofAddr := os.Getenv("PPROF_ADDR")
		if pprofAddr == "" {
			pprofAddr = ":6062"
		}
		runtime.SetMutexProfileFraction(10)
		runtime.SetBlockProfileRate(100_000_000)
		go func() {
			log.Printf("pprof debug server listening on %s", pprofAddr)
			if err := http.ListenAndServe(pprofAddr, nil); err != nil {
				log.Printf("pprof server error: %v", err)
			}
		}()
	}
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("load config: %v", err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	db, err := database.Connect(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatalf("connect database: %v", err)
	}
	defer db.Close()

	if err := database.RunMigrations(ctx, db, filepath.Join("migrations")); err != nil {
		log.Fatalf("run migrations: %v", err)
	}

	repo := billing.NewRepository(db.Pool)

	// Initialize Redis cache (optional — gracefully degraded when disabled)
	var redisClient *rediscache.Client
	if cfg.Redis.Enabled {
		log.Println("🔌 Connecting to Redis (billing-core)...")
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
			log.Println("✅ Redis cache connected (billing-core)")
		}
	}

	timeout := time.Duration(cfg.AdapterTimeoutSeconds) * time.Second
	paymentAdapter := buildPaymentAdapter(cfg, timeout)
	lagoAdapter := lago.NewAdapter(lago.Config{
		BaseURL: cfg.LagoBaseURL,
		APIKey:  cfg.LagoAPIKey,
		Timeout: timeout,
	})
	billingService := billing.NewService(repo, paymentAdapter, lagoAdapter, redisClient)

	// Wire shared cross-plane publisher (velion-nats) independently from
	// local controlplane-nats so cross-plane propagation still works if the
	// local broker is temporarily unavailable.
	if sp, spErr := nats.NewSharedPublisher(cfg.NATSSharedURL, nats.SharedCredentials{
		User: cfg.NATSSharedUser, Password: cfg.NATSSharedPass,
		Token: cfg.NATSSharedToken, AllowTokenFallback: cfg.NATSSharedAllowTokenFallback,
	}, cfg.ServiceName); spErr != nil {
		log.Fatalf("shared NATS unavailable: %v", spErr)
	} else if sp != nil {
		defer sp.Close()
		billingService.SetSharedPublisher(sp)
		log.Println("✅ billing-core connected to shared NATS (velion-nats)")
	}

	go billingService.StartRetryProcessor(ctx, billing.RetryProcessorConfig{
		PollInterval: time.Duration(cfg.RetryPollSeconds) * time.Second,
		BatchSize:    cfg.RetryBatchSize,
		MaxAttempts:  cfg.RetryMaxAttempts,
		BaseBackoff:  time.Duration(cfg.RetryBackoffSeconds) * time.Second,
	})

	// Trial-expiry sweep: revert organizations whose 14-day Pro trial elapsed
	// back to their base plan. Interval via BILLING_TRIAL_SWEEP_SECONDS (default
	// 3600, floor 60).
	go func() {
		intervalSec := 3600
		if v := strings.TrimSpace(os.Getenv("BILLING_TRIAL_SWEEP_SECONDS")); v != "" {
			if n, convErr := strconv.Atoi(v); convErr == nil && n >= 60 {
				intervalSec = n
			}
		}
		ticker := time.NewTicker(time.Duration(intervalSec) * time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				if n, sweepErr := billingService.ExpireTrials(ctx); sweepErr != nil {
					log.Printf("billing-core trial sweep error: %v", sweepErr)
				} else if n > 0 {
					log.Printf("billing-core expired %d trial(s)", n)
				}
			}
		}
	}()

	var publisher billing.EventPublisher
	natsClient, err := nats.NewClient(nats.Config{
		URL:   cfg.NATSURL,
		Token: cfg.NATSToken,
		Name:  cfg.ServiceName,
	})
	if err != nil {
		log.Printf("warning: nats unavailable, running without event bridge: %v", err)
	}

	if natsClient != nil {
		defer natsClient.Close()
		publisher = nats.NewPublisher(natsClient)
		billingService.SetPublisher(publisher)

		subscriber := nats.NewSubscriber(natsClient, billingService)
		if err := subscriber.Start(ctx); err != nil {
			log.Printf("warning: nats subscriber failed to start: %v", err)
		}
	}

	server := httpserver.NewServer(cfg.HTTPPort, billingService)
	grpcServer := grpcserver.NewServer(cfg.GRPCPort)

	// Prometheus /metrics on a dedicated port (default 9091), scraped by the
	// Control-Plane Prometheus (Phase 6 B13).
	metricsPort := 9091
	if v := strings.TrimSpace(os.Getenv("METRICS_PORT")); v != "" {
		if n, convErr := strconv.Atoi(v); convErr == nil && n > 0 {
			metricsPort = n
		}
	}
	metricsServer := metricsserver.NewServer(metricsPort)

	errCh := make(chan error, 3)
	go func() {
		if err := server.Start(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			errCh <- fmt.Errorf("http server: %w", err)
		}
	}()
	go func() {
		if err := grpcServer.Start(); err != nil {
			errCh <- fmt.Errorf("gRPC server: %w", err)
		}
	}()
	go func() {
		if err := metricsServer.Start(); err != nil {
			errCh <- fmt.Errorf("metrics server: %w", err)
		}
	}()

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt, syscall.SIGTERM)

	select {
	case sig := <-sigCh:
		log.Printf("shutdown signal received: %s", sig.String())
	case srvErr := <-errCh:
		if srvErr != nil {
			log.Printf("server stopped: %v", srvErr)
		}
	}

	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	if err := grpcServer.Shutdown(shutdownCtx); err != nil && !errors.Is(err, context.DeadlineExceeded) && !errors.Is(err, context.Canceled) {
		log.Printf("grpc shutdown error: %v", err)
	}
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("http shutdown error: %v", err)
	}
	if err := metricsServer.Shutdown(shutdownCtx); err != nil {
		log.Printf("metrics shutdown error: %v", err)
	}
}

func buildPaymentAdapter(cfg *config.Config, timeout time.Duration) billing.PaymentAdapter {
	provider := strings.ToLower(strings.TrimSpace(cfg.PaymentProvider))
	if provider == "" || provider == "auto" {
		if strings.TrimSpace(cfg.NexiSecretKey) != "" {
			provider = "nexi"
		} else if strings.TrimSpace(cfg.HyperswitchAPIKey) != "" || strings.TrimSpace(cfg.HyperswitchPublishableKey) != "" {
			provider = "hyperswitch"
		} else {
			provider = "stripe"
		}
	}

	switch provider {
	case "nexi":
		log.Println("billing-core payment provider: nexi")
		return nexi.NewAdapter(nexi.Config{
			BaseURL:              cfg.NexiBaseURL,
			SecretKey:            cfg.NexiSecretKey,
			CheckoutKey:          cfg.NexiCheckoutKey,
			CheckoutJSURL:        cfg.NexiCheckoutJSURL,
			WebhookURL:           cfg.NexiWebhookURL,
			WebhookAuthorization: cfg.NexiWebhookAuthorization,
			TermsURL:             cfg.NexiTermsURL,
			Timeout:              timeout,
		})
	case "hyperswitch":
		log.Println("billing-core payment provider: hyperswitch")
		return hyperswitch.NewAdapter(hyperswitch.Config{
			BaseURL:        cfg.HyperswitchBaseURL,
			APIKey:         cfg.HyperswitchAPIKey,
			PublishableKey: cfg.HyperswitchPublishableKey,
			ProfileID:      cfg.HyperswitchProfileID,
			ClientURL:      cfg.HyperswitchClientURL,
			BackendURL:     cfg.HyperswitchBackendURL,
			Timeout:        timeout,
		})
	case "stripe":
		log.Println("billing-core payment provider: stripe")
		return stripe.NewAdapter(stripe.Config{
			BaseURL: cfg.StripeBaseURL,
			APIKey:  cfg.StripeAPIKey,
			Timeout: timeout,
		})
	default:
		log.Printf("billing-core unknown PAYMENT_PROVIDER=%q; falling back to stripe", cfg.PaymentProvider)
		return stripe.NewAdapter(stripe.Config{
			BaseURL: cfg.StripeBaseURL,
			APIKey:  cfg.StripeAPIKey,
			Timeout: timeout,
		})
	}
}
