package main

import (
	"context"
	"log/slog"
	"os"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"

	"github.com/triodelab/model-plane/services/capability-core/internal/erasure"
)

// orgErasureEnabledEnv names the switch that binds the organization erasure consumer. Only the literal true turns it
// on; off, nothing here connects or purges, and capability-core behaves as it did before the consumer existed.
const orgErasureEnabledEnv = "CAPABILITY_CORE_ORG_ERASURE_ENABLED"

// The consumer runs over its own connection to the shared cross-plane broker (control-shared-nats), under a narrowly
// scoped identity. It is deliberately separate from the Model-Plane-local NATS_URL connection, which never carries the
// Control Plane's erasure subject, and its inbox prefix keeps it apart from that connection's.
const (
	orgErasureNATSURLEnv      = "CAPABILITY_CORE_GDPR_NATS_URL"
	orgErasureNATSUserEnv     = "CAPABILITY_CORE_GDPR_NATS_USER"
	orgErasureNATSPasswordEnv = "CAPABILITY_CORE_GDPR_NATS_PASSWORD"
	orgErasureInboxPrefix     = "_INBOX.CAPABILITY_CORE_GDPR"
)

// orgErasureEnabled reports whether the organization erasure consumer may start. Only the literal true enables it.
func orgErasureEnabled(lookup func(string) string) bool {
	return lookup(orgErasureEnabledEnv) == "true"
}

// orgErasureTarget returns the shared-broker URL the consumer connects to, and whether it should start at all. Off, it
// starts nothing and reads no URL. On with no URL, it starts nothing, and the caller says so.
func orgErasureTarget(lookup func(string) string) (string, bool) {
	if !orgErasureEnabled(lookup) {
		return "", false
	}
	natsURL := strings.TrimSpace(lookup(orgErasureNATSURLEnv))
	return natsURL, natsURL != ""
}

// startOrgErasureConsumer starts the organization erasure consumer in the background when the switch reads the literal
// true and a broker URL is set. It returns at once; the consumer runs until ctx is done. A switch that is on without a
// URL logs the gap and starts nothing, so the rest of the service is unaffected.
func startOrgErasureConsumer(ctx context.Context, pool *pgxpool.Pool) {
	natsURL, ok := orgErasureTarget(os.Getenv)
	if !ok {
		if orgErasureEnabled(os.Getenv) {
			slog.Error("org erasure consumer not started: " + orgErasureNATSURLEnv + " is unset")
		}
		return
	}
	go runOrgErasureConsumer(ctx, natsURL, erasure.NewPgPurger(pool))
}

// orgErasureBindRetry is how long the consumer waits before it tries to bind again. A bind can fail while the broker is
// still coming up, or while the durable is not yet provisioned, and an erasure that never binds is a privacy gap, so the
// consumer keeps trying until the service stops.
const orgErasureBindRetry = 10 * time.Second

// runOrgErasureConsumer connects to the shared broker, binds the pre-provisioned durable and waits until ctx is done.
func runOrgErasureConsumer(ctx context.Context, natsURL string, purger erasure.Purger) {
	nc, err := nats.Connect(natsURL, orgErasureNATSOptions(os.Getenv)...)
	if err != nil {
		slog.Error("shared-broker NATS connect failed; org erasure consumer disabled", "error", err)
		return
	}
	defer nc.Close()

	js, err := nc.JetStream()
	if err != nil {
		slog.Error("shared-broker JetStream context failed; org erasure consumer disabled", "error", err)
		return
	}
	consumer := erasure.NewOrgErasureConsumer(js, purger)
	for {
		err := consumer.Start()
		if err == nil {
			break
		}
		slog.Error("org erasure consumer bind failed; retrying (is capability-core-org-erasure pre-provisioned on control-shared-nats?)",
			"error", err, "retry_in", orgErasureBindRetry.String())
		select {
		case <-ctx.Done():
			return
		case <-time.After(orgErasureBindRetry):
		}
	}
	defer consumer.Stop()

	slog.Info("org erasure consumer active")
	<-ctx.Done()
}

// orgErasureNATSOptions sets the reconnect policy and the inbox prefix of the shared-broker connection, and its identity
// when one is configured.
func orgErasureNATSOptions(lookup func(string) string) []nats.Option {
	options := []nats.Option{
		nats.RetryOnFailedConnect(true),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(2 * time.Second),
		nats.CustomInboxPrefix(orgErasureInboxPrefix),
	}
	user := strings.TrimSpace(lookup(orgErasureNATSUserEnv))
	password := strings.TrimSpace(lookup(orgErasureNATSPasswordEnv))
	if user != "" || password != "" {
		return append(options, nats.UserInfo(user, password))
	}
	return options
}
