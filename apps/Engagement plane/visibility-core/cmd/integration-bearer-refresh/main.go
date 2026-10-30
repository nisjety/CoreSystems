package main

import (
	"context"
	"errors"
	"flag"
	"log"
	"os"
	"os/signal"
	"syscall"

	"github.com/triodelab/engagement-plane/visibility-core/internal/bearerrefresh"
)

func main() {
	once := flag.Bool("once", false, "issue one org-scoped bearer and exit")
	check := flag.Bool("check", false, "check bearer binding and expiry without issuing a token")
	bundle := flag.Bool("studio-bundle", false, "renew the four fixed Studio audience bearers in a private directory")
	flag.Parse()
	if *once && *check {
		log.Fatal("choose either --once or --check")
	}
	if *bundle {
		configs, err := studioBundleConfigs(configFromEnvironment(), os.Getenv("STUDIO_BEARER_DIRECTORY"), !*check)
		if err != nil {
			log.Fatalf("Studio bearer bundle config: %v", err)
		}
		workers, err := newBundleWorkers(configs, nil, nil)
		if err != nil {
			log.Fatalf("Studio bearer bundle config: %v", err)
		}
		ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
		defer stop()
		if err := runStudioBundle(ctx, workers, *once, *check, func(name string, state bearerrefresh.State, err error) {
			if err != nil {
				log.Printf("Studio bearer profile %s is not ready", name)
				return
			}
			log.Printf("Studio bearer profile %s expires %s", name, state.ExpiresAt.Format("2006-01-02T15:04:05Z07:00"))
		}); err != nil && !errors.Is(err, context.Canceled) {
			log.Fatal(err)
		}
		return
	}
	refresher, err := bearerrefresh.New(configFromEnvironment(), nil, nil)
	if err != nil {
		log.Fatalf("Studio service bearer config: %v", err)
	}
	if *check {
		if _, err := refresher.Check(); err != nil {
			log.Fatal("Studio service bearer is not ready")
		}
		return
	}
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	if *once {
		if _, err := refresher.Refresh(ctx); err != nil {
			log.Fatalf("Studio service bearer: %v", err)
		}
		log.Print("Studio service bearer issued")
		return
	}
	if err := refresher.Run(ctx, func(state bearerrefresh.State, err error) {
		if err != nil {
			log.Printf("Studio service bearer renewal: %v", err)
			return
		}
		log.Printf("Studio service bearer renewed; expires %s", state.ExpiresAt.Format("2006-01-02T15:04:05Z07:00"))
	}); err != nil && !errors.Is(err, context.Canceled) {
		log.Fatalf("Studio service bearer: %v", err)
	}
}

func configFromEnvironment() bearerrefresh.Config {
	return bearerrefresh.Config{
		AuthCoreURL:    os.Getenv("AUTH_CORE_URL"),
		OrganizationID: environmentAlias("STUDIO_ORGANIZATION_ID", "MARKETING_INTEGRATION_ORGANIZATION_ID"),
		ServiceID:      environmentAlias("STUDIO_SERVICE_ID", "MARKETING_INTEGRATION_SERVICE_ID"),
		ServiceKeyFile: environmentAlias("STUDIO_SERVICE_KEY_FILE", "MARKETING_INTEGRATION_SERVICE_KEY_FILE"),
		TokenFile:      environmentAlias("STUDIO_BEARER_TOKEN_FILE", "INTEGRATION_BEARER_TOKEN_FILE"),
		Capability:     os.Getenv("STUDIO_BEARER_CAPABILITY"),
	}
}

func environmentAlias(modern, legacy string) string {
	if value, present := os.LookupEnv(modern); present {
		return value
	}
	return os.Getenv(legacy)
}
