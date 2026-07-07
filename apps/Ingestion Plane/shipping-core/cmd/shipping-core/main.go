// Command shipping-core is the entrypoint for the shipping-core backend:
// HTTP API, carrier quote fan-out, and (in later phases) booking, Visma.net
// sync, and reliability scoring. See docs/ARCHITECTURE.md for the system
// design this implements.
package main

import (
	"context"
	"errors"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/go-chi/chi/v5"

	db "shipping-core/db"
	"shipping-core/internal/booking"
	"shipping-core/internal/carrier"
	"shipping-core/internal/carrier/bring"
	"shipping-core/internal/carrier/dhl"
	"shipping-core/internal/carrier/fedex"
	"shipping-core/internal/carrier/mock"
	"shipping-core/internal/carrier/ups"
	"shipping-core/internal/platform"
	"shipping-core/internal/quoteengine"
)

const perCarrierTimeout = 4 * time.Second

func main() {
	logger := platform.NewLogger()

	if err := run(logger); err != nil {
		logger.Error("fatal", "err", err.Error())
		os.Exit(1)
	}
}

func run(logger *slog.Logger) error {
	databaseURL := os.Getenv("DATABASE_URL")
	if databaseURL == "" {
		return errors.New("DATABASE_URL is required")
	}
	port := os.Getenv("PORT")
	if port == "" {
		port = "8080"
	}

	if err := db.Migrate(databaseURL); err != nil {
		return err
	}
	logger.Info("migrations applied")

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	pool, err := platform.NewDBPool(ctx, databaseURL)
	if err != nil {
		return err
	}
	defer pool.Close()

	adapters := buildCarriers(logger)
	quoters := make([]quoteengine.Quoter, len(adapters))
	for i, a := range adapters {
		quoters[i] = a
	}
	engine := quoteengine.New(quoters, perCarrierTimeout)

	router := chi.NewRouter()
	router.Use(platform.RequestLogger(logger))
	router.Get("/healthz", platform.HealthzHandler())
	router.Get("/readyz", platform.ReadyzHandler(pool))
	router.Post("/api/quotes", quoteengine.Handler(engine))
	router.Get("/api/carriers", quoteengine.CarriersHandler(engine))

	// Booking lifecycle: two-step confirmation gate, labels, pickups,
	// tracking persistence, manifests, audit trail.
	bookingStore := booking.NewStore(pool)
	bookingSvc := booking.NewService(bookingStore, adapters, logger)
	booking.Routes(router, bookingSvc, bookingStore)

	srv := &http.Server{
		Addr:              ":" + port,
		Handler:           router,
		ReadHeaderTimeout: 5 * time.Second,
	}

	serveErr := make(chan error, 1)
	go func() {
		logger.Info("listening", "port", port)
		if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			serveErr <- err
		}
	}()

	select {
	case err := <-serveErr:
		return err
	case <-ctx.Done():
		logger.Info("shutting down")
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		return srv.Shutdown(shutdownCtx)
	}
}

// buildCarriers assembles the adapter fleet: mock adapters by default,
// with each real integration activated individually when its credentials
// are present in the environment. Real Bring/UPS/FedEx replace their mock
// counterparts (where one exists) so the comparison table never shows the
// same carrier twice. Missing credentials are logged at info level and
// skipped — never a startup error, since carrier API access arrives
// piecemeal as agreements are signed (docs/TASKS.md Fase 0).
func buildCarriers(logger *slog.Logger) []carrier.Adapter {
	adapters := mock.DefaultCarriers()

	if cfg, err := bring.NewConfigFromEnv(); err != nil {
		logger.Info("bring credentials not configured, using mock adapter", "reason", err.Error())
	} else {
		adapters = replaceCarrier(adapters, "mock-bring", bring.New(cfg))
		logger.Info("bring credentials found, using real adapter")
	}

	if cfg, err := dhl.NewConfigFromEnv(); err != nil {
		logger.Info("dhl credentials not configured, using mock adapter", "reason", err.Error())
	} else {
		adapters = replaceCarrier(adapters, "mock-dhl", dhl.New(cfg))
		logger.Info("dhl credentials found, using real adapter", "base_url", cfg.BaseURL)
	}

	if cfg, err := ups.NewConfigFromEnv(); err != nil {
		logger.Info("ups credentials not configured, skipping", "reason", err.Error())
	} else {
		adapters = append(adapters, ups.New(cfg))
		logger.Info("ups credentials found, using real adapter", "base_url", cfg.BaseURL)
	}

	if cfg, err := fedex.NewConfigFromEnv(); err != nil {
		logger.Info("fedex credentials not configured, skipping", "reason", err.Error())
	} else {
		adapters = append(adapters, fedex.New(cfg))
		logger.Info("fedex credentials found, using real adapter", "base_url", cfg.BaseURL)
	}

	return adapters
}

// replaceCarrier swaps out the adapter with the given code for a real
// implementation, appending the replacement at the end.
func replaceCarrier(adapters []carrier.Adapter, code string, replacement carrier.Adapter) []carrier.Adapter {
	kept := make([]carrier.Adapter, 0, len(adapters)+1)
	for _, a := range adapters {
		if a.Info().Code == code {
			continue
		}
		kept = append(kept, a)
	}
	return append(kept, replacement)
}
