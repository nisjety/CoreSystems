package main

import (
	"errors"
	"log/slog"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/triodelab/model-plane/services/browser-broker/internal/keyrevoke"
	bbserver "github.com/triodelab/model-plane/services/browser-broker/internal/server"
)

// INNH-10 MP-2: the api_key_revoked consumer, on its own NATS connection.
const (
	envKeyRevokeNatsURL      = "BROWSER_BROKER_KEY_REVOKE_NATS_URL"
	envKeyRevokeNatsUser     = "BROWSER_BROKER_KEY_REVOKE_NATS_USER"
	envKeyRevokeNatsPassword = "BROWSER_BROKER_KEY_REVOKE_NATS_PASSWORD"
	envKeyRevokeStream       = "BROWSER_BROKER_KEY_REVOKE_STREAM"
)

// errKeyRevokeUnconfigured: key grants on, but nothing would ever revoke them
// when their key is revoked.
var errKeyRevokeUnconfigured = errors.New(envKeyRevokeNatsURL + " is required when BROWSER_BROKER_KEY_GRANTS_ENABLED is on")

// keyRevokeGate refuses to start a broker that issues key grants without a
// revoke consumer configured.
func keyRevokeGate(cfg bbserver.Config, getenv func(string) string) error {
	if cfg.KeyGrantsEnabled && strings.TrimSpace(getenv(envKeyRevokeNatsURL)) == "" {
		return errKeyRevokeUnconfigured
	}
	return nil
}

// startKeyRevokeConsumer connects and binds the consumer when key grants are
// on. If it cannot, key grants are disabled for this process (fail closed for
// the feature, the rest of the broker keeps serving). The returned stop is
// never nil.
func startKeyRevokeConsumer(cfg bbserver.Config, getenv func(string) string, srv *bbserver.Server) func() {
	if !cfg.KeyGrantsEnabled {
		return func() {}
	}
	url := strings.TrimSpace(getenv(envKeyRevokeNatsURL))
	options := []nats.Option{
		nats.Name("browser-broker-key-revoke"),
		nats.Timeout(5 * time.Second),
		nats.MaxReconnects(-1),
	}
	if user := getenv(envKeyRevokeNatsUser); user != "" {
		options = append(options, nats.UserInfo(user, getenv(envKeyRevokeNatsPassword)))
	}
	nc, err := nats.Connect(url, options...)
	if err != nil {
		// Never log the URL: it can carry credentials.
		slog.Error("api key revoke consumer cannot connect; key grants are disabled for this process")
		srv.DisableKeyGrants()
		return func() {}
	}
	js, err := nc.JetStream()
	if err != nil {
		slog.Error("api key revoke consumer has no JetStream; key grants are disabled for this process")
		srv.DisableKeyGrants()
		nc.Close()
		return func() {}
	}
	consumer := keyrevoke.New(js, getenv(envKeyRevokeStream), srv)
	if err := consumer.Start(); err != nil {
		slog.Error("api key revoke consumer cannot bind its durable; key grants are disabled for this process",
			"durable", keyrevoke.Durable, "error", err.Error())
		srv.DisableKeyGrants()
		nc.Close()
		return func() {}
	}
	return func() {
		consumer.Stop()
		nc.Close()
	}
}
