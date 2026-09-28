package main

import (
	"context"
	"log"
	"net/http"
	"os"
	"os/signal"
	"time"

	"coresystem/learning-core/internal/auth"
	learninghttp "coresystem/learning-core/internal/http"
	"coresystem/learning-core/internal/learning"
)

func main() {
	addr := os.Getenv("LEARNING_LISTEN_ADDR")
	if addr == "" {
		addr = "127.0.0.1:3196"
	}
	if err := learninghttp.ValidateListenAddress(addr); err != nil {
		log.Fatal(err)
	}
	handler := learninghttp.NewHandler()
	authURL := os.Getenv("AUTH_SERVICE_URL")
	membershipToken := os.Getenv("LEARNING_CORE_MEMBERSHIP_SERVICE_TOKEN")
	databaseURL := os.Getenv("LEARNING_DATABASE_URL")
	migrationURL := os.Getenv("LEARNING_MIGRATION_DATABASE_URL")
	configuration := []string{authURL, membershipToken, databaseURL, migrationURL}
	configured, missing := false, false
	for _, value := range configuration {
		configured = configured || value != ""
		missing = missing || value == ""
	}
	if configured && missing {
		log.Fatal("school access and course storage require AUTH_SERVICE_URL, LEARNING_CORE_MEMBERSHIP_SERVICE_TOKEN, LEARNING_DATABASE_URL, and LEARNING_MIGRATION_DATABASE_URL together")
	}
	if configured {
		authority, err := auth.NewAuthority(authURL, membershipToken)
		if err != nil {
			log.Fatalf("configure Control Plane authority: %v", err)
		}
		startup, cancelStartup := context.WithTimeout(context.Background(), 15*time.Second)
		repository, err := learning.Open(startup, databaseURL, migrationURL)
		cancelStartup()
		if err != nil {
			log.Fatalf("configure Learning storage: %v", err)
		}
		defer repository.Close()
		handler = learning.NewHandler(authority, repository)
		log.Printf("Learning Core connected to Control authority and Learning-owned storage; speech and guided teaching remain disabled")
	}
	server := &http.Server{Addr: addr, Handler: handler, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 10 * time.Second, WriteTimeout: 10 * time.Second, IdleTimeout: 30 * time.Second}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	go func() {
		<-ctx.Done()
		shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := server.Shutdown(shutdown); err != nil {
			log.Printf("shutdown: %v", err)
		}
	}()
	if !configured {
		log.Printf("learning-core foundation listening on %s; school auth and durable learning operations remain disabled", addr)
	}
	if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		log.Fatal(err)
	}
}
