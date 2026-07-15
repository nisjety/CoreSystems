package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/triodelab/controlplane/audit-core/internal/provisioner"
)

type bridgeConfig struct {
	SourceURL      string
	SourceUser     string
	SourcePassword string
	LegacyURL      string
	LegacyToken    string
}

func loadConfig() (bridgeConfig, error) {
	config := bridgeConfig{
		SourceURL:      strings.TrimSpace(os.Getenv("CONTROL_SHARED_NATS_URL")),
		SourceUser:     strings.TrimSpace(os.Getenv("CONTROL_SHARED_BRIDGE_USER")),
		SourcePassword: strings.TrimSpace(os.Getenv("CONTROL_SHARED_BRIDGE_PASSWORD")),
		LegacyURL:      strings.TrimSpace(os.Getenv("VELION_LEGACY_NATS_URL")),
		LegacyToken:    strings.TrimSpace(os.Getenv("VELION_LEGACY_NATS_TOKEN")),
	}
	if config.SourceURL == "" || config.LegacyURL == "" || config.SourceUser == "" {
		return bridgeConfig{}, errors.New("source URL/user and legacy URL are required")
	}
	if len(config.SourcePassword) < 32 || len(config.LegacyToken) < 32 {
		return bridgeConfig{}, errors.New("bridge credentials must contain at least 32 characters")
	}
	return config, nil
}

func main() {
	config, err := loadConfig()
	if err != nil {
		log.Fatal(err)
	}
	source, err := nats.Connect(config.SourceURL,
		nats.UserInfo(config.SourceUser, config.SourcePassword),
		nats.CustomInboxPrefix("_INBOX.CONTROL_SHARED_BRIDGE"),
		nats.Name("control-shared-legacy-bridge-source"),
		nats.MaxReconnects(-1),
	)
	if err != nil {
		log.Fatal(err)
	}
	defer source.Close()
	legacy, err := nats.Connect(config.LegacyURL,
		nats.Token(config.LegacyToken),
		nats.Name("control-shared-legacy-bridge-target"),
		nats.MaxReconnects(-1),
	)
	if err != nil {
		log.Fatal(err)
	}
	defer legacy.Close()
	subscription, err := startBridge(source, legacy)
	if err != nil {
		log.Fatal(err)
	}
	defer subscription.Unsubscribe() //nolint:errcheck
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	<-ctx.Done()
}

func startBridge(source, legacy *nats.Conn) (*nats.Subscription, error) {
	js, err := source.JetStream()
	if err != nil {
		return nil, fmt.Errorf("open source JetStream: %w", err)
	}
	legacyJS, err := legacy.JetStream()
	if err != nil {
		return nil, fmt.Errorf("open legacy JetStream: %w", err)
	}
	subscription, err := js.QueueSubscribe(
		">",
		provisioner.LegacyBridgeConsumerName,
		func(message *nats.Msg) {
			if err := forwardToLegacy(legacy, legacyJS, message); err != nil {
				_ = message.NakWithDelay(time.Second)
				return
			}
			_ = message.Ack()
		},
		nats.Bind(provisioner.ControlSharedStreamName, provisioner.LegacyBridgeConsumerName),
	)
	if err != nil {
		return nil, fmt.Errorf("bind compatibility bridge consumer: %w", err)
	}
	return subscription, nil
}

func forwardToLegacy(
	legacy *nats.Conn,
	legacyJS nats.JetStreamContext,
	source *nats.Msg,
) error {
	// GDPR subjects contain security/deletion evidence and have completed their
	// scoped producer/consumer migration. They must never be copied onto the
	// compatibility broker where a legacy shared token grants broader access.
	if strings.HasPrefix(source.Subject, "velion.gdpr.") {
		return nil
	}
	header := nats.Header{}
	for key, values := range source.Header {
		header[key] = append([]string(nil), values...)
	}
	if header.Get(nats.MsgIdHdr) == "" {
		metadata, err := source.Metadata()
		if err != nil {
			return fmt.Errorf("read source JetStream identity: %w", err)
		}
		header.Set(
			nats.MsgIdHdr,
			fmt.Sprintf("control-shared:%s:%d", metadata.Stream, metadata.Sequence.Stream),
		)
	}
	target := &nats.Msg{
		Subject: source.Subject,
		Header:  header,
		Data:    append([]byte(nil), source.Data...),
	}
	// Notification Core intentionally consumes core-NATS subjects and the
	// legacy broker has no notification stream. Preserve that transport while
	// using target PubAck/de-duplication for durable domain/session subjects.
	if strings.HasPrefix(source.Subject, "notifications.") {
		if err := legacy.PublishMsg(target); err != nil {
			return fmt.Errorf("publish legacy core notification: %w", err)
		}
		return legacy.FlushTimeout(5 * time.Second)
	}
	ack, err := legacyJS.PublishMsg(target)
	if err != nil {
		return fmt.Errorf("publish legacy compatibility event: %w", err)
	}
	if ack == nil || ack.Stream == "" || ack.Sequence == 0 {
		return errors.New("invalid legacy compatibility PubAck")
	}
	return nil
}
