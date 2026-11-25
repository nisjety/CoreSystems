package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/http"
	_ "net/http/pprof"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/browsersites"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/clients"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	grpcserver "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/grpc"
	httpserver "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/http"
	metricsserver "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/metrics"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/modelentitlement"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/nats"
	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/orgkeys"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/rbac"
	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/redis"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
)

func main() {
	if err := httpserver.ValidateRequiredServiceCredentialRegistry(os.Getenv("ORG_CORE_SERVICE_CREDENTIALS")); err != nil {
		log.Fatalf("[org-core startup] service credential registry validation failed: %v", err)
	}
	log.Printf("[org-core startup] scoped service credential registry OK")

	if err := httpserver.ValidateSIEMDestinationsConfig(os.Getenv); err != nil {
		log.Fatalf("[org-core startup] SIEM destinations validation failed: %v", err)
	}

	// WS17.E2 org key service. Everything below is read only with the switch on; the
	// refusal names the switch and never the key material or any env value.
	orgKeysOn := orgkeys.Enabled(os.Getenv)
	if orgKeysOn {
		if err := httpserver.ValidateOrgKeysConfig(os.Getenv); err != nil {
			log.Fatalf("[org-core startup] %s is on but the org key master keys are not usable; refusing to start", orgkeys.EnvEnabled)
		}
		log.Printf("[org-core startup] org key master keys OK")
	}

	// pprof debug server — enable with PPROF_ENABLED=true; default addr :6061
	if os.Getenv("PPROF_ENABLED") == "true" {
		pprofAddr := os.Getenv("PPROF_ADDR")
		if pprofAddr == "" {
			pprofAddr = ":6061"
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

	repo := orgcore.NewRepository(db)
	// U6-3 (ui-ux-verevon-gap.md §10): role/permission editor backend.
	// NewRepository takes *database.DB (not db.Pool) so every rbac query
	// runs through WithOrgScope — see internal/rbac/repository.go.
	rbacRepo := rbac.NewRepository(db)

	// Initialize Redis cache (optional — gracefully degraded when disabled)
	var redisClient *rediscache.Client
	if cfg.Redis.Enabled {
		log.Println("🔌 Connecting to Redis (org-core)...")
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
			log.Println("✅ Redis cache connected (org-core)")
		}
	}

	var publisher orgcore.Publisher
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
	}

	orgService := orgcore.NewService(repo, publisher, redisClient)
	if publisher != nil {
		go runPlanChangeOutbox(ctx, orgService, 5*time.Second)
	}

	// Wire the local control-plane JetStream publisher. Audit intents are first
	// committed to PostgreSQL, then published to the pre-provisioned Control
	// observability stream with a stable Nats-Msg-Id and validated PubAck.
	if natsClient != nil {
		orgService.SetAuditPublisher(natsClient)
		go runGDPRAuditOutbox(ctx, orgService, 5*time.Second)
		go runSIEMAuditOutbox(ctx, orgService, 5*time.Second)
		go runIdentityAuditOutbox(ctx, orgService, 5*time.Second)
		go runOrgFlagAuditOutbox(ctx, orgService, 5*time.Second)
	}

	// Wire shared cross-plane publisher (verevon-nats)
	if sp, spErr := nats.NewSharedPublisher(cfg.NATSSharedURL, nats.SharedCredentials{
		User: cfg.NATSSharedUser, Password: cfg.NATSSharedPass,
		Token: cfg.NATSSharedToken, AllowTokenFallback: cfg.NATSSharedAllowTokenFallback,
	}, cfg.ServiceName); spErr != nil {
		log.Fatalf("shared NATS unavailable: %v", spErr)
	} else if sp != nil {
		defer sp.Close()
		orgService.SetSharedPublisher(sp)
		log.Println("✅ org-core connected to shared NATS (verevon-nats)")
		go runInteractiveRetentionOutbox(ctx, orgService, 5*time.Second)
	}

	var subscriber *nats.BridgeSubscriber
	if natsClient != nil {
		subscriber = nats.NewBridgeSubscriber(natsClient, publisher.(*nats.Publisher), orgService)
		if err := subscriber.Start(ctx); err != nil {
			log.Printf("warning: nats bridge subscriber failed to start: %v", err)
		}
	}

	// Mirrors billing-core's trial/checkout-driven plan changes into
	// organizations.plan — see BillingSyncSubscriber's doc comment for why
	// this direction had no consumer at all before.
	var billingSyncSubscriber *nats.BillingSyncSubscriber
	if natsClient != nil {
		billingSyncSubscriber = nats.NewBillingSyncSubscriber(natsClient, orgService)
		if err := billingSyncSubscriber.Start(ctx); err != nil {
			log.Printf("warning: billing sync subscriber failed to start: %v", err)
		}
	}

	// GDPR retention sweep: hard-delete organizations soft-deleted more than
	// ORG_PURGE_DAYS days ago (default 30) by calling the
	// purge_old_deleted_organizations stored procedure. Runs once at startup,
	// then daily, and exits when ctx is cancelled. Mirrors audit-core's
	// retention cron shape. Started below, once the retention service that
	// records a held organisation's skip (F-f10-06) is wired.

	// Flow C reminder sweep: 7-day and 1-day-out reminders for organizations
	// pending deletion. Runs once at startup, then every
	// orgDeletionReminderInterval, and exits when ctx is cancelled. Mirrors
	// runOrgPurge/runGDPRAuditOutbox's ticker shape.
	go runOrgDeletionReminderSweep(ctx, orgService, orgDeletionReminderInterval)

	server := httpserver.NewServer(cfg.HTTPPort, orgService, rbacRepo, cfg.AuthServiceURL, cfg.UserServiceURL)
	// Chat model policy (WS10-S2, ADR-0005). Roles come from the membership
	// table through orgService, never from a request header.
	server.SetModelEntitlements(modelentitlement.NewService(modelentitlement.NewRepository(db), orgService))
	// Agent-browser site policy (WS21-S3). Read by the browser broker at grant mint;
	// administered and chosen through the /api/v1 browser-sites routes.
	browserSites := browsersites.NewService(browsersites.NewRepository(db), time.Now)
	server.SetBrowserSites(browserSites)
	// F-f06-11: every site policy and choice write queues a content-free audit intent when
	// ORG_BROWSER_SITE_AUDIT_ENABLED is the literal "true". The switch is read once here. The
	// publisher loop below runs whatever it says, so intents already queued still drain.
	browserSites.SetAuditEnabled(browsersites.AuditEnabled(os.Getenv))
	if natsClient != nil {
		browserSites.SetAuditPublisher(natsClient)
		go runBrowserSiteAuditOutbox(ctx, browserSites, 5*time.Second)
	}
	// F-f03-02: a Space-scoped resolve reads the Space's browser floor from user-core with the
	// server's own user-core URL and service token. A missing URL or token leaves the floor
	// unread, so a Space-scoped resolve refuses every host.
	server.SetBrowserSpaceFloor()
	// A membership that ends takes its remembered site choices with it (F-f02-16).
	orgService.SetBrowserSiteChoiceEraser(browserSites)
	// Retention policy and legal holds (EP-9 C1); read by session-core's sweeper. Thread holds ask
	// session-core's thread receipt first when ORG_RETENTION_THREAD_RECEIPT_CHECK is the literal true
	// (F-f10-03); off, they are placed without the check, as before. The reader is nil unless the
	// switch is on, so the service never calls a client it was not given.
	// The typed-nil guard matters: a nil *nats.Client inside the interface is non-nil.
	threadReceipts, err := threadReceiptFromEnvironment(os.Getenv)
	if err != nil {
		log.Fatalf("[org-core startup] %v", err)
	}
	var retentionPublisher retention.AuditPublisher
	if natsClient != nil {
		retentionPublisher = natsClient
	}
	retentionService := retention.NewService(retention.NewRepository(db), threadReceipts, retentionPublisher)
	server.SetRetention(retentionService)
	// The purge sweep (see above) records each organisation it skips for a legal hold
	// (ERASURE_HOLD_CHECK, F-f10-06), so it starts only after its auditor is set.
	orgService.SetErasureRefusalAuditor(retentionService)
	go runOrgPurge(ctx, orgService, orgPurgeDays())
	if natsClient != nil {
		go runRetentionAuditOutbox(ctx, retentionService, 5*time.Second)
	}
	// Org key service (WS17.E2). Off: nothing is read, built or started here, and the
	// key routes are not mounted. On: a store or master-key failure refuses boot.
	if orgKeysOn {
		orgKeysService, err := orgkeys.NewServiceFromEnv(db, os.Getenv)
		if err != nil {
			log.Fatalf("[org-core startup] %s is on but the org key service could not be built; refusing to start", orgkeys.EnvEnabled)
		}
		// Same typed-nil guard as the retention publisher above.
		if natsClient != nil {
			orgKeysService.SetAuditPublisher(natsClient)
		}
		server.SetOrgKeys(orgKeysService)
		go runOrgKeysAuditOutbox(ctx, orgKeysService, 5*time.Second)
	}
	grpcServer := grpcserver.NewServer(cfg.GRPCPort)
	metricsServer := metricsserver.NewServer(cfg.MetricsPort)

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
		if err := metricsServer.Start(); err != nil && !errors.Is(err, http.ErrServerClosed) {
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
	grpcServer.Stop()
	if err := metricsServer.Shutdown(shutdownCtx); err != nil {
		log.Printf("metrics shutdown error: %v", err)
	}
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("http shutdown error: %v", err)
	}
}

func runPlanChangeOutbox(ctx context.Context, service *orgcore.Service, interval time.Duration) {
	flush := func() {
		published, err := service.FlushPlanChangeOutbox(ctx, 100)
		if err != nil {
			log.Printf("org-core plan change outbox retry failed: %v", err)
			return
		}
		if published > 0 {
			log.Printf("org-core published %d pending plan change event(s)", published)
		}
	}
	flush()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			flush()
		}
	}
}

func runInteractiveRetentionOutbox(ctx context.Context, service *orgcore.Service, interval time.Duration) {
	flush := func() {
		published, err := service.FlushInteractiveRetentionOutbox(ctx, 100)
		if err != nil {
			log.Printf("org-core interactive retention outbox retry failed: %v", err)
			return
		}
		if published > 0 {
			log.Printf("org-core published %d interactive retention cleanup event(s)", published)
		}
	}
	flush()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			flush()
		}
	}
}

func runGDPRAuditOutbox(ctx context.Context, service *orgcore.Service, interval time.Duration) {
	flush := func() {
		result, err := service.FlushGDPRAuditOutbox(ctx, 100)
		if err != nil {
			log.Printf("org-core GDPR audit outbox retry failed: %v", err)
		}
		if result.Published > 0 || result.DeadLettered > 0 {
			log.Printf(
				"org-core GDPR audit outbox published=%d dead_lettered=%d",
				result.Published, result.DeadLettered,
			)
		}
	}
	flush()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			flush()
		}
	}
}

// runSIEMAuditOutbox publishes the audit rows of SIEM destination changes. It runs whatever
// ORG_SIEM_DESTINATIONS_ENABLED says, so rows still pending when the switch is turned off drain.
func runSIEMAuditOutbox(ctx context.Context, service *orgcore.Service, interval time.Duration) {
	flush := func() {
		result, err := service.FlushSIEMAuditOutbox(ctx, 100)
		if err != nil {
			log.Printf("org-core SIEM audit outbox retry failed: %v", err)
		}
		if result.Published > 0 || result.DeadLettered > 0 {
			log.Printf(
				"org-core SIEM audit outbox published=%d dead_lettered=%d",
				result.Published, result.DeadLettered,
			)
		}
	}
	flush()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			flush()
		}
	}
}

// runRetentionAuditOutbox publishes the audit rows of retention policy and legal hold changes
// (EP-9 C1). It runs whatever ORG_RETENTION_POLICY_ENABLED says, so rows still pending when the
// switch is turned off drain.
func runRetentionAuditOutbox(ctx context.Context, service *retention.Service, interval time.Duration) {
	flush := func() {
		result, err := service.FlushAuditOutbox(ctx, 100)
		if err != nil {
			log.Printf("org-core retention audit outbox retry failed: %v", err)
		}
		if result.Published > 0 || result.DeadLettered > 0 {
			log.Printf(
				"org-core retention audit outbox published=%d dead_lettered=%d",
				result.Published, result.DeadLettered,
			)
		}
	}
	flush()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			flush()
		}
	}
}

// threadReceiptFromEnvironment builds session-core's thread receipt reader for the thread hold check
// (F-f10-03). Off, the switch returns a nil reader and builds and calls nothing, as before. On, it refuses to
// start unless the reader can be built: a thread hold must never be placed without the check the switch asked
// for. The reader calls SessionRetentionService.GetThreadRetention (F-f10-02) on the Model Plane session-core
// at SESSION_CORE_RETENTION_ADDR, with a session-core token minted for each organisation read from org-core's
// own service API key. A refusal names the switch and no value.
func threadReceiptFromEnvironment(getenv func(string) string) (retention.ThreadRetention, error) {
	return threadReceiptReader(getenv, clients.NewSessionCoreTokenSource, clients.NewSessionThreadRetentionClient)
}

// The variables the reader is built from. The API key is a service credential: it is read here and never logged.
const (
	envSessionCoreRetentionAddr = "SESSION_CORE_RETENTION_ADDR"
	envOrgCoreSessionAPIKey     = "ORG_CORE_SESSION_SERVICE_API_KEY"
	envAuthCoreURL              = "AUTH_CORE_URL"
)

// tokenSourceBuilder and clientBuilder are the two constructors the reader is built from. Tests substitute
// counting fakes to show that a switch that is off builds neither.
type tokenSourceBuilder func(authCoreURL, serviceID, credential string) (clients.TokenSource, error)

type clientBuilder func(addr string, tokens clients.TokenSource) (*clients.SessionThreadRetentionClient, error)

// threadReceiptReader is threadReceiptFromEnvironment with its constructors passed in.
func threadReceiptReader(getenv func(string) string, newTokens tokenSourceBuilder, newClient clientBuilder) (retention.ThreadRetention, error) {
	if !retention.ThreadReceiptCheckEnabled(getenv) {
		return nil, nil
	}
	refuse := func(reason string) error {
		return fmt.Errorf("%s is on but %s; refusing to start", retention.EnvThreadReceiptCheck, reason)
	}
	addr := strings.TrimSpace(getenv(envSessionCoreRetentionAddr))
	authCoreURL := strings.TrimSpace(getenv(envAuthCoreURL))
	apiKey := strings.TrimSpace(getenv(envOrgCoreSessionAPIKey))
	switch {
	case addr == "":
		return nil, refuse(envSessionCoreRetentionAddr + " is unset")
	case authCoreURL == "":
		return nil, refuse(envAuthCoreURL + " is unset")
	case apiKey == "":
		return nil, refuse(envOrgCoreSessionAPIKey + " is unset")
	}
	tokens, err := newTokens(authCoreURL, clients.OrgCoreServiceID, apiKey)
	if err != nil {
		return nil, refuse(fmt.Sprintf("the session-core token source could not be built (%v)", err))
	}
	client, err := newClient(addr, tokens)
	if err != nil {
		return nil, refuse(fmt.Sprintf("the session-core thread receipt client could not be built (%v)", err))
	}
	return client, nil
}

// runIdentityAuditOutbox publishes the audit rows of identity changes (domain added, verified,
// revoked; WS05-S6). It runs whatever ORG_DOMAIN_VERIFICATION_ENABLED says, so rows still
// pending when the switch is turned off drain.
func runIdentityAuditOutbox(ctx context.Context, service *orgcore.Service, interval time.Duration) {
	flush := func() {
		result, err := service.FlushIdentityAuditOutbox(ctx, 100)
		if err != nil {
			log.Printf("org-core identity audit outbox retry failed: %v", err)
		}
		if result.Published > 0 || result.DeadLettered > 0 {
			log.Printf(
				"org-core identity audit outbox published=%d dead_lettered=%d",
				result.Published, result.DeadLettered,
			)
		}
	}
	flush()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			flush()
		}
	}
}

// orgFlagAuditFlusher is the seam runOrgFlagAuditOutbox drives. *orgcore.Service satisfies it.
type orgFlagAuditFlusher interface {
	FlushOrgFlagAuditOutbox(ctx context.Context, limit int) (orgcore.GDPRAuditFlushResult, error)
}

// runOrgFlagAuditOutbox publishes the audit intents of organisation flag writes (W5-079-b).
// SetOrgFlag flushes once after each commit, but a publish that fails there stays pending, and
// nothing else retries it until the next flag write in any organisation (P2-cp-06-rel1). So this
// loop flushes on its own ticks. It runs whatever ORG_FLAGS_ENABLED says, so rows still pending
// when the switch is turned off drain. Off, the routes write no row, so a flush finds nothing new.
func runOrgFlagAuditOutbox(ctx context.Context, service orgFlagAuditFlusher, interval time.Duration) {
	flush := func() {
		result, err := service.FlushOrgFlagAuditOutbox(ctx, 100)
		if err != nil {
			log.Printf("org-core org flag audit outbox retry failed: %v", err)
		}
		if result.Published > 0 || result.DeadLettered > 0 {
			log.Printf(
				"org-core org flag audit outbox published=%d dead_lettered=%d",
				result.Published, result.DeadLettered,
			)
		}
	}
	flush()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			flush()
		}
	}
}

// runBrowserSiteAuditOutbox publishes the audit intents of agent-browser site policy writes and
// member choice writes (F-f06-11). It runs whatever ORG_BROWSER_SITE_AUDIT_ENABLED says, so rows
// still pending when the switch is turned off drain. Logs carry counts only, never a host.
func runBrowserSiteAuditOutbox(ctx context.Context, service *browsersites.Service, interval time.Duration) {
	flush := func() {
		result, err := service.FlushBrowserSiteAuditOutbox(ctx, 100)
		if err != nil {
			log.Printf("org-core browser site audit outbox retry failed: %v", err)
		}
		if result.Published > 0 || result.DeadLettered > 0 {
			log.Printf(
				"org-core browser site audit outbox published=%d dead_lettered=%d",
				result.Published, result.DeadLettered,
			)
		}
	}
	flush()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			flush()
		}
	}
}

// runOrgKeysAuditOutbox publishes the pending audit intents of the org key service
// (WS17.E2). It runs only with ORG_CORE_KEYS_ENABLED on. Logs carry fixed text and
// counts only, never an error string, so no key id, purpose or env value can leak.
// A failing flush (no NATS publisher, store error) is logged once when the loop
// starts failing and again only after it recovers, so a missing publisher does not
// flood the log every tick.
func runOrgKeysAuditOutbox(ctx context.Context, service *orgkeys.Service, interval time.Duration) {
	failing := false
	flush := func() {
		published, err := service.FlushAudit(ctx, 100)
		if err != nil {
			if !failing && ctx.Err() == nil {
				log.Printf("org-core org key audit outbox flush failed; intents stay pending until it succeeds")
			}
			failing = true
			return
		}
		if failing {
			log.Printf("org-core org key audit outbox flush recovered")
			failing = false
		}
		if published > 0 {
			log.Printf("org-core org key audit outbox published=%d", published)
		}
	}
	flush()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			flush()
		}
	}
}

// orgPurgeInterval is how often the GDPR retention sweep runs. Daily by design
// — purging is a maintenance task, not a hot path.
const orgPurgeInterval = 24 * time.Hour

// orgPurgeDays reads ORG_PURGE_DAYS (default 30). Values below 1 are clamped to
// 1 so a misconfiguration can never purge everything on the next sweep.
func orgPurgeDays() int {
	days := 30
	if v := strings.TrimSpace(os.Getenv("ORG_PURGE_DAYS")); v != "" {
		var d int
		if _, err := fmt.Sscanf(v, "%d", &d); err == nil && d > 0 {
			days = d
		} else {
			log.Printf("warning: invalid ORG_PURGE_DAYS=%q; using default %d", v, days)
		}
	}
	if days < 1 {
		days = 1
	}
	return days
}

// runOrgPurge invokes purge_old_deleted_organizations(days) once immediately,
// then daily, logging the purged count. Returns when ctx is cancelled.
func runOrgPurge(ctx context.Context, svc *orgcore.Service, days int) {
	purge := func() {
		// Bound each sweep so a slow purge can't block shutdown indefinitely.
		sweepCtx, cancel := context.WithTimeout(ctx, 5*time.Minute)
		defer cancel()

		receipt, err := svc.PurgeDeletedOrganizations(sweepCtx, days)
		if err != nil {
			log.Printf("org GDPR purge failed (days=%d): %v", days, err)
			return
		}
		var parsed struct {
			PurgedCount int `json:"purged_count"`
		}
		_ = json.Unmarshal(receipt, &parsed)
		log.Printf("org GDPR purge complete: purged_count=%d days=%d", parsed.PurgedCount, days)
	}

	purge() // immediate sweep at startup
	ticker := time.NewTicker(orgPurgeInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			log.Println("org GDPR purge loop stopping")
			return
		case <-ticker.C:
			purge()
		}
	}
}

// orgDeletionReminderInterval is how often the Flow C deletion-reminder sweep
// runs. A few hours is frequent enough that a 7-day/1-day-out reminder never
// slips by more than a few hours, without hammering the DB like a hot-path
// query would.
const orgDeletionReminderInterval = 6 * time.Hour

// runOrgDeletionReminderSweep finds organizations pending deletion whose
// 30-day grace window is 7 (or 1) days from expiring and have not yet had
// that reminder sent, publishes verevon.org.deletion.reminder for each, and
// marks the reminder sent so the next sweep does not re-fire it — the
// idempotency guard for this at-least-once ticker. Runs once immediately,
// then on orgDeletionReminderInterval, and returns when ctx is cancelled.
func runOrgDeletionReminderSweep(ctx context.Context, svc *orgcore.Service, interval time.Duration) {
	sweepKind := func(which string, daysRemaining int, list func(context.Context) ([]orgcore.OrgPendingDeletionReminder, error)) {
		sweepCtx, cancel := context.WithTimeout(ctx, 5*time.Minute)
		defer cancel()

		reminders, err := list(sweepCtx)
		if err != nil {
			log.Printf("org-core deletion reminder sweep (%s) failed to list candidates: %v", which, err)
			return
		}

		sent := 0
		for _, reminder := range reminders {
			svc.PublishDeletionReminder(sweepCtx, reminder.OrgID, reminder.OrgName, daysRemaining)
			if err := svc.MarkReminderSent(sweepCtx, reminder.OrgID, which); err != nil {
				log.Printf("org-core deletion reminder sweep (%s): mark sent failed for org=%s: %v", which, reminder.OrgID, err)
				continue
			}
			sent++
		}
		if sent > 0 {
			log.Printf("org-core sent %d %s deletion reminder(s)", sent, which)
		}
	}

	sweep := func() {
		sweepKind("7d", 7, svc.ListOrgsNeeding7DayReminder)
		sweepKind("1d", 1, svc.ListOrgsNeeding1DayReminder)
	}

	sweep() // immediate sweep at startup
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			log.Println("org-core deletion reminder sweep loop stopping")
			return
		case <-ticker.C:
			sweep()
		}
	}
}
