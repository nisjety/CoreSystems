package main

import (
	"context"
	"errors"
	stdlog "log"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/logger"
	"github.com/gofiber/fiber/v2/middleware/recover"
	"github.com/gofiber/fiber/v2/middleware/requestid"
	"github.com/joho/godotenv"
	zlog "github.com/rs/zerolog/log"

	quarrybrowser "github.com/triodelab/quarry/internal/browser"
	"github.com/triodelab/quarry/internal/config"
	"github.com/triodelab/quarry/internal/scraper"
	"github.com/triodelab/quarry/internal/session"
)

func main() {
	if err := godotenv.Load(); err != nil {
		zlog.Warn().Err(err).Msg("No .env file found (using defaults/environment)")
	}

	cfg, err := config.Load()
	if err != nil {
		stdlog.Fatalf("failed to load config: %v", err)
	}

	app := fiber.New(fiber.Config{
		ErrorHandler: func(c *fiber.Ctx, err error) error {
			var fiberErr *fiber.Error
			status := fiber.StatusInternalServerError
			message := "internal server error"
			if errors.As(err, &fiberErr) {
				status = fiberErr.Code
				if strings.TrimSpace(fiberErr.Message) != "" {
					message = fiberErr.Message
				}
			}
			return c.Status(status).JSON(fiber.Map{
				"success": false,
				"error":   message,
			})
		},
	})
	app.Use(recover.New())
	app.Use(requestid.New())
	app.Use(logger.New(logger.Config{
		Format: "${time} ${status} - ${latency} ${method} ${path} reqid=${locals:requestid}\n",
	}))

	pool, err := scraper.NewBrowserPool(cfg.BrowserPoolSize, true)
	if err != nil {
		stdlog.Fatalf("failed to initialize browser pool: %v", err)
	}
	sessionMgr := session.NewManager(pool, session.DefaultConfig())
	runtime := quarrybrowser.NewLocalRuntime(sessionMgr)
	quarrybrowser.NewServer(runtime, cfg.BrowserServiceInternalAPIKey).Register(app)

	port := strings.TrimSpace(os.Getenv("BROWSER_SERVICE_PORT"))
	if port == "" {
		port = strings.TrimSpace(os.Getenv("PORT"))
	}
	if port == "" {
		port = "8091"
	}

	serverErrCh := make(chan error, 1)
	go func() {
		serverErrCh <- app.Listen(":" + port)
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, os.Interrupt, syscall.SIGTERM)
	select {
	case <-stop:
	case err := <-serverErrCh:
		if err != nil {
			zlog.Error().Err(err).Msg("browser service stopped unexpectedly")
		}
		return
	}

	zlog.Info().Msg("shutting down browser service gracefully...")

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := app.ShutdownWithContext(shutdownCtx); err != nil {
		zlog.Error().Err(err).Msg("browser service shutdown failed")
	}
	if err := <-serverErrCh; err != nil {
		zlog.Debug().Err(err).Msg("browser service loop exited after shutdown")
	}
	if err := sessionMgr.Close(); err != nil {
		zlog.Error().Err(err).Msg("session manager shutdown failed")
	}
	if err := pool.Close(); err != nil {
		zlog.Error().Err(err).Msg("browser pool shutdown failed")
	}

	zlog.Info().Msg("browser service shutdown complete")
}
