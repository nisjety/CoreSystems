package main

import (
	"context"
	"log/slog"
	"os"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
	"github.com/triodelab/model-plane/services/capability-core/internal/eventtrigger"
)

// startEventTriggerFire wires the EP-7 W4-F T2 fire path. The sweeper decides pending fires with the same fresh
// Control authorizer the cron sweeper uses, so nothing is decided without one. With EVENT_TRIGGER_RUN_ENABLED the
// starter also begins a scheduled run for each authorised fire. The consumer also needs
// EVENT_TRIGGER_NATS_URL and a durable the ingestion stream owner provisioned. Without the URL only the sweeper runs,
// and it decides nothing until a fire has been recorded.
func startEventTriggerFire(ctx context.Context, pool *pgxpool.Pool, authorizer *cron.ControlFireAuthorizer, authErr error) {
	if authErr != nil {
		slog.Warn("event trigger fire not started; fresh Control authorization is required", "error", authErr)
		return
	}
	sweeper, err := eventtrigger.NewSweeper(pool, authorizer, eventtrigger.LimitsFromEnv(os.Getenv))
	if err != nil {
		slog.Warn("event trigger sweeper not started", "error", err)
		return
	}
	// EP-7 W4-F T5: the wake gate reads a filtered fire's webhook payload from integration-corev2 and refuses the fire when
	// the trigger's filter does not match. Strict literal true; off, stored filters are ignored and every fire is decided as
	// before. It needs integration-corev2's URL and internal key as well.
	if os.Getenv("EVENT_TRIGGER_WAKE_GATE_ENABLED") == "true" {
		source := eventtrigger.NewIntegrationWebhookSource(os.Getenv("EVENT_TRIGGER_IC_URL"), os.Getenv("EVENT_TRIGGER_IC_API_KEY"))
		if !source.Configured() {
			slog.Warn("event trigger wake gate has no integration-corev2 URL or key; filtered fires stay pending and are refused")
		}
		sweeper.EnableWakeGate(source)
		slog.Info("event trigger wake gate enabled")
	}
	go sweeper.Start(ctx)
	slog.Info("event trigger sweeper started with fresh Control authorization")

	// EP-7 W4-F T4: the starter turns each authorised fire of a trigger that names a schedule into one scheduled run,
	// through the cron fire path. Strict literal true, in addition to EVENT_TRIGGER_FIRE_ENABLED; off by default.
	// The workflow dispatcher asks Control again before each run, so the starter needs no authorizer of its own.
	if os.Getenv("EVENT_TRIGGER_RUN_ENABLED") == "true" {
		starter, err := eventtrigger.NewStarter(pool)
		if err != nil {
			slog.Warn("event trigger starter not started", "error", err)
		} else {
			go starter.Start(ctx)
			slog.Info("event trigger starter started")
		}
	}

	url := strings.TrimSpace(os.Getenv("EVENT_TRIGGER_NATS_URL"))
	if url == "" {
		slog.Warn("event trigger consumer not started; EVENT_TRIGGER_NATS_URL is not set")
		return
	}
	consumer, err := eventtrigger.NewConsumer(pool)
	if err != nil {
		slog.Warn("event trigger consumer not started", "error", err)
		return
	}
	go func() {
		nc, err := connectEventTriggerNATS(url)
		if err != nil {
			slog.Error("event trigger consumer not started; NATS is unavailable", "error", err)
			return
		}
		js, err := nc.JetStream()
		if err != nil {
			slog.Error("event trigger consumer not started; JetStream context unavailable", "error", err)
			return
		}
		if err := consumer.Run(ctx, js); err != nil {
			slog.Warn("event trigger consumer stopped (is it pre-provisioned on "+eventtrigger.WebhookStream+"?)", "error", err)
		}
	}()
}

// connectEventTriggerNATS dials the broker that carries the ingestion stream with its own credentials. It retries the
// way connectNATSWithRetry does, because that broker may come up after this process.
func connectEventTriggerNATS(url string) (*nats.Conn, error) {
	options := []nats.Option{nats.CustomInboxPrefix(natsInboxPrefix), nats.Name("capability-core-event-triggers")}
	if user := strings.TrimSpace(os.Getenv("EVENT_TRIGGER_NATS_USER")); user != "" {
		options = append(options, nats.UserInfo(user, os.Getenv("EVENT_TRIGGER_NATS_PASSWORD")))
	}
	var err error
	for attempt := 1; attempt <= 60; attempt++ {
		var nc *nats.Conn
		nc, err = nats.Connect(url, options...)
		if err == nil {
			return nc, nil
		}
		slog.Warn("event trigger NATS connect attempt failed; retrying", "attempt", attempt, "error", err)
		time.Sleep(time.Second)
	}
	return nil, err
}
