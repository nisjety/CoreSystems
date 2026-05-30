package main

import (
	"context"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/controlplane"
	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/eventing"
	httpserver "github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/http"
	natsclient "github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/nats"
	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/runtime"
	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/workspace"
)

func main() {
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

	repository := workspace.NewRepository(db.Pool)

	var publisher *eventing.Publisher
	natsConn, err := natsclient.NewClient(natsclient.Config{
		URL:   cfg.NATSURL,
		Token: cfg.NATSToken,
		Name:  cfg.ServiceName,
	})
	if err != nil {
		log.Printf("affine-core: NATS unavailable: %v", err)
	} else {
		defer natsConn.Close()
		publisher = eventing.NewPublisher(natsConn)
	}

	runtimeClient := runtime.NewClient(runtime.Config{
		BaseURL:       cfg.AffineRuntimeURL,
		AdminEmail:    cfg.AffineAdminEmail,
		AdminPassword: cfg.AffineAdminPassword,
	})

	controlPlaneClient := controlplane.NewClient(controlplane.Config{
		AuthServiceURL: cfg.AuthServiceURL,
		UserServiceURL: cfg.UserServiceURL,
		InternalAPIKey: cfg.InternalAPIKey,
	})

	workspaceService := workspace.NewService(repository, publisher)
	handler := httpserver.NewHandler(cfg, runtimeClient, workspaceService, controlPlaneClient)
	server := httpserver.NewServer(cfg.HTTPPort, handler, cfg.InternalAPIKey)

	errCh := make(chan error, 1)
	go func() {
		errCh <- server.Start()
	}()

	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt, syscall.SIGTERM)

	select {
	case sig := <-sigCh:
		log.Printf("shutdown signal received: %s", sig)
	case srvErr := <-errCh:
		if srvErr != nil {
			log.Printf("server error: %v", srvErr)
		}
	}

	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("shutdown error: %v", err)
	}
}
