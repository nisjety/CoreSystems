package main

import (
	"context"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/go-chi/chi/v5"
	chimw "github.com/go-chi/chi/v5/middleware"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
	"google.golang.org/grpc"

	wikipb "github.com/triodelab/dataplane/gen/go/wiki/v1"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/config"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/eventauth"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/events"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/gdpr"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/grpcserver"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/handler"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/metrics"
	apmotel "github.com/triodelab/dataplane/services/wiki-store-go/internal/otel"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/repo"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "wiki-store-go").Logger()

	cfg := config.Load()
	if err := cfg.ValidateEventSecurity(); err != nil {
		log.Fatal().Err(err).Msg("wiki event security configuration invalid")
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audience:      cfg.JWTAudience,
		Issuer:        cfg.JWTIssuer,
		PublicKeyFile: cfg.JWTPublicKeyFile,
		JWKSURL:       cfg.JWKSURL,
	})
	if err != nil {
		log.Fatal().Err(err).Msg("JWT verification configuration invalid")
	}
	authMiddleware := authctx.Middleware(verifier)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	otelShutdown, err := apmotel.Init(ctx, "wiki-store-go")
	if err != nil {
		log.Warn().Err(err).Msg("otel init failed, continuing without tracing")
	} else {
		defer otelShutdown(ctx)
	}

	pool, err := pgxpool.New(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatal().Err(err).Msg("postgres connect failed")
	}
	defer pool.Close()

	if err := pool.Ping(ctx); err != nil {
		log.Fatal().Err(err).Msg("postgres ping failed")
	}

	wikiRepo := repo.NewWikiRepo(pool)

	// §16.3.8 — attach the mandatory signed NATS publisher so CreatePage /
	// CreateVersion emit `dataplane.wiki.version.published`. Production
	// posture was validated above and cannot silently disable this boundary.
	if cfg.NatsURL != "" {
		privateKey, err := os.ReadFile(cfg.EventSigningPrivateKeyPath)
		if err != nil {
			log.Fatal().Err(err).Msg("wiki event signing key unavailable")
		}
		signer, err := eventauth.NewSigner(privateKey)
		if err != nil {
			log.Fatal().Err(err).Msg("wiki event signing key invalid")
		}
		natsOptions := []nats.Option{nats.Name("wiki-store")}
		if cfg.NatsToken != "" {
			natsOptions = append(natsOptions, nats.Token(cfg.NatsToken))
		}
		nc, err := nats.Connect(cfg.NatsURL, natsOptions...)
		if err != nil {
			log.Fatal().Err(err).Msg("NATS connect failed")
		}
		defer nc.Close()
		publisher, err := events.NewPublisher(nc, signer)
		if err != nil {
			log.Fatal().Err(err).Msg("acknowledged wiki publisher initialization failed")
		}
		outbox, err := events.NewOutboxPublisher(pool, publisher)
		if err != nil {
			log.Fatal().Err(err).Msg("wiki outbox initialization failed")
		}
		outbox.Start(ctx)
		log.Info().Msg("signed acknowledged wiki outbox publisher attached")
	}

	// GDPR cross-plane org-erasure purge consumer — subscribes to
	// verevon.gdpr.erasure.requested (org-core, fanned out over the shared
	// control-shared-nats broker) and hard-purges this org's wiki +
	// Operating Map data. Uses a SECOND, narrowly-scoped "wiki-store-gdpr"
	// shared-broker connection (WIKISTORE_GDPR_SHARED_NATS_URL/_USER/
	// _PASSWORD) kept entirely separate from cfg.NatsURL above (the
	// Data-Plane-local broker the wiki-event publisher uses) — see
	// config.go's field doc for why this consumer's env var names are
	// deliberately distinct from any name already claimed in this service's
	// own NatsURL fallback chain. Safe by default: absent
	// WIKISTORE_GDPR_SHARED_NATS_URL, the consumer is simply not started;
	// org deletions will not auto-purge this service's wiki data until it is
	// configured.
	var gdprNC *nats.Conn
	if strings.TrimSpace(cfg.GDPRSharedNatsURL) != "" {
		nc, err := nats.Connect(cfg.GDPRSharedNatsURL,
			nats.Name("wiki-store-gdpr"),
			nats.UserInfo(cfg.GDPRSharedNatsUser, cfg.GDPRSharedNatsPassword),
			nats.CustomInboxPrefix("_INBOX.WIKI_STORE_GDPR"),
		)
		if err != nil {
			log.Warn().Err(err).Msg("control-shared NATS connect failed — GDPR org-erasure purge consumer disabled")
		} else if _, err := gdpr.StartSubscriber(nc, wikiRepo); err != nil {
			log.Warn().Err(err).Msg("GDPR org-erasure subscriber failed to start")
			nc.Close()
		} else {
			gdprNC = nc
		}
	} else {
		log.Warn().Msg("WIKISTORE_GDPR_SHARED_NATS_URL unset — GDPR org-erasure purge consumer disabled (org deletions will not auto-purge this service's wiki data for that org)")
	}
	defer func() {
		if gdprNC != nil {
			gdprNC.Close()
		}
	}()

	wikiHandler := handler.NewWikiHandler(wikiRepo)

	r := chi.NewRouter()
	r.Use(chimw.RequestID)
	r.Use(chimw.RealIP)
	r.Use(chimw.Recoverer)
	r.Use(chimw.Timeout(30 * time.Second))
	r.Use(metrics.Middleware)

	r.Get("/health", handler.Health)
	r.Get("/readyz", handler.Readyz)
	r.Method("GET", "/metrics", metrics.Handler())

	handler.MountRoutes(r, authMiddleware, wikiHandler)

	addr := fmt.Sprintf("0.0.0.0:%d", cfg.HTTPPort)
	srv := &http.Server{Addr: addr, Handler: r}

	go func() {
		log.Info().Str("addr", addr).Msg("wiki-store-go HTTP starting")
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatal().Err(err).Msg("http server error")
		}
	}()

	// gRPC server on a separate port — same WikiRepo backs both wires.
	grpcAddr := fmt.Sprintf("0.0.0.0:%d", cfg.GRPCPort)
	grpcLis, gerr := net.Listen("tcp", grpcAddr)
	if gerr != nil {
		log.Fatal().Err(gerr).Str("addr", grpcAddr).Msg("grpc listen failed")
	}
	grpcSrv := grpc.NewServer(grpc.UnaryInterceptor(authctx.UnaryServerInterceptor(verifier)))
	wikipb.RegisterWikiServiceServer(grpcSrv, grpcserver.New(wikiRepo))
	go func() {
		log.Info().Str("addr", grpcAddr).Msg("wiki-store-go gRPC starting")
		if err := grpcSrv.Serve(grpcLis); err != nil {
			log.Error().Err(err).Msg("grpc server error")
		}
	}()
	defer grpcSrv.GracefulStop()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit

	log.Info().Msg("shutting down")
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	srv.Shutdown(shutdownCtx)
}
