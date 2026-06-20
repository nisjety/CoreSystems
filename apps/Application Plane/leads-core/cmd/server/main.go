package main

import (
	"context"
	"errors"
	"log"
	stdhttp "net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/brreg"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/database"
	apphttp "github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/http"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/leads"
)

func main() {
	ctx := context.Background()
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("leads-core: config: %v", err)
	}

	db, err := database.Connect(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatalf("leads-core: database: %v", err)
	}
	defer db.Close()
	if err := database.RunMigrations(ctx, db); err != nil {
		log.Fatalf("leads-core: migrations: %v", err)
	}

	repository := leads.NewRepository(db.Pool)
	service := leads.NewService(repository, brreg.NewClient())
	handler := apphttp.NewHandler(cfg, service)
	server := apphttp.NewServer(cfg.HTTPPort, handler, cfg.InternalAPIKey)

	errCh := make(chan error, 1)
	go func() {
		errCh <- server.Start()
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, syscall.SIGINT, syscall.SIGTERM)

	select {
	case sig := <-stop:
		log.Printf("leads-core: received %s", sig)
	case err := <-errCh:
		if err != nil && !errors.Is(err, stdhttp.ErrServerClosed) {
			log.Fatalf("leads-core: server: %v", err)
		}
	}

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := server.Shutdown(shutdownCtx); err != nil {
		log.Printf("leads-core: shutdown: %v", err)
	}
}
