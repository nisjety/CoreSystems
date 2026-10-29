// Command shipping-core is the entrypoint for the shipping-core backend:
// HTTP API, carrier quote fan-out, and (in later phases) booking, Visma.net
// sync, and reliability scoring. See docs/ARCHITECTURE.md for the system
// design this implements.
package main

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/go-chi/chi/v5"

	db "shipping-core/db"
	shippingauth "shipping-core/internal/auth"
	"shipping-core/internal/booking"
	"shipping-core/internal/capabilityhealth"
	"shipping-core/internal/carrier"
	"shipping-core/internal/carrier/bring"
	"shipping-core/internal/carrier/dhl"
	"shipping-core/internal/carrier/fedex"
	"shipping-core/internal/carrier/mock"
	"shipping-core/internal/carrier/ups"
	"shipping-core/internal/dataplane"
	"shipping-core/internal/events"
	"shipping-core/internal/modelplane"
	"shipping-core/internal/platform"
	"shipping-core/internal/quoteengine"
	"shipping-core/internal/recommend"
	"shipping-core/internal/reliability"
)

const perCarrierTimeout = 4 * time.Second

// trackingRefreshInterval bounds how often the periodic tracking-refresh
// ticker polls carriers for open shipments. Deliveries don't happen faster
// than this matters, and it keeps carrier API load bounded regardless of
// booking volume.
const trackingRefreshInterval = 30 * time.Minute

// trackingRefreshWindow is RefreshOpenTracking's booked_at lookback — see
// its doc comment for why bounding matters.
const trackingRefreshWindow = 60 * 24 * time.Hour

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
	reliabilityStore := reliability.NewStore(pool)
	modelPlaneClient := modelplane.New(modelplane.NewConfigFromEnv())
	eventPublisher := buildEventPublisher(logger)
	dataPlaneClient := dataplane.New(dataplane.NewConfigFromEnv())
	bookingStore := booking.NewStore(pool)
	bookingSvc := booking.NewService(bookingStore, adapters, logger)
	bookingSvc.SetDeliveryObserver(&deliveryHooks{events: eventPublisher, dataPlane: dataPlaneClient, logger: logger})
	authConfig, err := shippingauth.ConfigFromEnv()
	if err != nil {
		return fmt.Errorf("load shipping authentication config: %w", err)
	}
	authMiddleware, err := shippingauth.NewMiddleware(authConfig)
	if err != nil {
		return fmt.Errorf("initialize shipping authentication: %w", err)
	}

	router := chi.NewRouter()
	router.Use(platform.RequestLogger(logger))
	router.Get("/healthz", platform.HealthzHandler())
	router.Get("/readyz", platform.ReadyzHandler(pool))
	router.Group(func(api chi.Router) {
		api.Use(authMiddleware)
		api.Post("/api/quotes", quoteengine.Handler(engine, reliabilityStore))
		api.Get("/api/carriers", quoteengine.CarriersHandler(engine))
		api.Get("/api/carriers/reliability", quoteengine.ReliabilityHandler(reliabilityScoresAdapter(reliabilityStore)))
		api.Post("/api/quotes/recommend", recommend.Handler(engine, reliabilityStore, modelPlaneClient, eventPublisher))

		// Booking lifecycle: two-step confirmation gate, labels, pickups,
		// tracking persistence, manifests, audit trail.
		booking.Routes(api, bookingSvc, bookingStore)
	})

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

	go runTrackingRefreshLoop(ctx, bookingSvc, logger)
	go capabilityhealth.RunHeartbeat(ctx, engine, logger)

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

// reliabilityScoresAdapter adapts reliability.Store.Scores (its own named
// CarrierScore type) to quoteengine.ScoresFunc (quoteengine's own minimal
// ScoreEntry type) — the wiring layer is the one place allowed to know
// both shapes, so neither package needs to import the other.
func reliabilityScoresAdapter(store *reliability.Store) quoteengine.ScoresFunc {
	return func(ctx context.Context) ([]quoteengine.ScoreEntry, error) {
		scores, err := store.Scores(ctx)
		if err != nil {
			return nil, err
		}
		out := make([]quoteengine.ScoreEntry, len(scores))
		for i, sc := range scores {
			out[i] = quoteengine.ScoreEntry{CarrierCode: sc.CarrierCode, OnTimeRate: sc.OnTimeRate, SampleSize: sc.SampleSize}
		}
		return out, nil
	}
}

// runTrackingRefreshLoop periodically calls RefreshOpenTracking so F8's
// reliability data accumulates even for bookings nobody opens the tracking
// page for. Ticks immediately on start (rather than waiting a full
// interval) so a freshly-deployed instance doesn't sit idle for 30 minutes
// before its first pass.
func runTrackingRefreshLoop(ctx context.Context, svc *booking.Service, logger *slog.Logger) {
	refresh := func() {
		rctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
		defer cancel()
		n, err := svc.RefreshOpenTracking(rctx, trackingRefreshWindow)
		if err != nil {
			logger.Warn("tracking refresh loop failed", "err", err.Error())
			return
		}
		logger.Info("tracking refresh loop completed", "refreshed", n)
	}

	refresh()
	ticker := time.NewTicker(trackingRefreshInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			refresh()
		}
	}
}

// buildEventPublisher connects to NATS_URL when set, falling back to
// events.NoopPublisher{} when unset or unreachable — a broker outage or a
// standalone dev run must never block quotes/bookings, matching every
// other optional cross-plane integration in this codebase.
func buildEventPublisher(logger *slog.Logger) events.Publisher {
	cfg := events.NewConfigFromEnv()
	if cfg.URL == "" {
		logger.Info("NATS_URL not set; booking/recommendation events will not be published")
		return events.NoopPublisher{}
	}
	if !unverifiedLegacyEventsEnabled(
		os.Getenv("ALLOW_UNVERIFIED_LEGACY_EVENTS"),
		os.Getenv("ALLOW_INSECURE_DEV_DEFAULTS"),
		os.Getenv("ISOLATED_E2E"),
	) {
		logger.Warn("unsigned shipping NATS events disabled in production posture")
		return events.NoopPublisher{}
	}
	pub, err := events.NewNATSPublisher(cfg, "shipping-core")
	if err != nil {
		logger.Warn("failed to connect to NATS; falling back to noop publisher", "err", err.Error())
		return events.NoopPublisher{}
	}
	return pub
}

func unverifiedLegacyEventsEnabled(legacy, insecure, isolated string) bool {
	return legacy == "1" && insecure == "1" && isolated == "1"
}

// deliveryHooks implements booking.DeliveryObserver, firing both
// cross-plane side effects on the first observation of delivery: an
// audit-trail NATS event (Control Plane alignment) and a Data Plane
// evidence document (F8's reliability facts made retrievable/citable via
// Verevon's knowledge surface). Both are best-effort — a failure here is
// logged, never surfaced to the tracking-refresh caller, since the
// booking's own delivery record already landed successfully.
type deliveryHooks struct {
	events    events.Publisher
	dataPlane *dataplane.Client
	logger    *slog.Logger
}

func (h *deliveryHooks) OnDelivered(ctx context.Context, rec booking.Record, deliveredAt time.Time) {
	if rec.ZDR {
		h.logger.Info("delivery persistence suppressed by ZDR", "booking", rec.ID)
		return
	}
	if rec.OrgID == "" {
		h.logger.Error("delivery persistence blocked: booking has no organization", "booking", rec.ID)
		return
	}
	onTime := rec.EstimatedDelivery == nil || deliveredAt.Format("2006-01-02") <= rec.EstimatedDelivery.Format("2006-01-02")

	if err := h.events.Publish(ctx, events.Event{
		Type:           "booking.delivered",
		OrganizationID: rec.OrgID,
		Data: map[string]any{
			"booking_id":   rec.ID,
			"carrier_code": rec.CarrierCode,
			"delivered_at": deliveredAt.Format(time.RFC3339),
			"on_time":      onTime,
		},
	}); err != nil {
		h.logger.Warn("publish booking.delivered event failed", "booking", rec.ID, "err", err.Error())
	}

	if !h.dataPlane.Configured() {
		return
	}
	verdict := "on time"
	if !onTime {
		verdict = "late"
	}
	estimated := "unknown"
	if rec.EstimatedDelivery != nil {
		estimated = rec.EstimatedDelivery.Format("2006-01-02")
	}
	metadata := map[string]any{
		"booking_id": rec.ID, "carrier_code": rec.CarrierCode, "on_time": onTime,
	}
	if rec.RetentionUntil != nil {
		metadata["retention_until"] = rec.RetentionUntil.Format(time.RFC3339)
	}
	content := fmt.Sprintf(
		"Shipment %s (booking %s) via %s was delivered %s on %s. Estimated delivery: %s.",
		rec.TrackingNo, rec.ID, rec.CarrierName, verdict, deliveredAt.Format("2006-01-02"), estimated,
	)
	if _, err := h.dataPlane.CreateDocument(ctx, dataplane.DocumentRequest{
		OrgID:          rec.OrgID,
		Source:         "shipping-core",
		Type:           "shipping-delivery",
		Title:          fmt.Sprintf("Delivery: %s via %s", rec.TrackingNo, rec.CarrierName),
		Content:        content,
		Metadata:       metadata,
		CreatedBy:      rec.BookedBy,
		IdempotencyKey: "shipping-delivery:" + rec.ID,
		IngestPolicy:   &dataplane.IngestPolicy{ZDRMode: "off", EphemeralOnly: false},
	}); err != nil {
		h.logger.Warn("push delivery evidence to data plane failed", "booking", rec.ID, "err", err.Error())
	}
}
