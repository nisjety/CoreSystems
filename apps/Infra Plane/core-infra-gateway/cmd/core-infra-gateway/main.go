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

	"github.com/coresystem/core-infra-gateway/internal/httpapi"
)

var version = "dev"

func main() {
	address := os.Getenv("CORE_INFRA_GATEWAY_ADDR")
	if address == "" {
		address = ":7500"
	}

	handler := httpapi.NewHandler(httpapi.Config{
		ServiceName:   "core-infra-gateway",
		Version:       version,
		OperatorToken: os.Getenv("CORE_INFRA_OPERATOR_TOKEN"),
	})
	server := &http.Server{
		Addr:              address,
		Handler:           handler,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       15 * time.Second,
		WriteTimeout:      15 * time.Second,
		IdleTimeout:       60 * time.Second,
	}

	errorsChannel := make(chan error, 1)
	go func() {
		slog.Info("Core Infra gateway listening", "address", address, "version", version)
		errorsChannel <- server.ListenAndServe()
	}()

	signals := make(chan os.Signal, 1)
	signal.Notify(signals, os.Interrupt, syscall.SIGTERM)

	select {
	case signal := <-signals:
		slog.Info("Core Infra gateway shutting down", "signal", signal.String())
		shutdownContext, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		if err := server.Shutdown(shutdownContext); err != nil {
			slog.Error("Core Infra gateway shutdown failed", "error", err)
			os.Exit(1)
		}
	case err := <-errorsChannel:
		if !errors.Is(err, http.ErrServerClosed) {
			slog.Error("Core Infra gateway stopped unexpectedly", "error", err)
			os.Exit(1)
		}
	}
}
