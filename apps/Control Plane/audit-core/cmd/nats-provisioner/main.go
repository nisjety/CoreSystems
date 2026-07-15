package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"os"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/triodelab/controlplane/audit-core/internal/provisioner"
)

func main() {
	buses, err := provisioner.ParseBuses(os.Getenv("AUDIT_NATS_PROVISION_BUSES"))
	if err != nil {
		log.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	if err := runProvisioning(ctx, buses, provisionBus); err != nil {
		log.Fatal(err)
	}
}

func runProvisioning(ctx context.Context, buses []provisioner.Bus, provision func(context.Context, provisioner.Bus) error) error {
	failures := make([]error, 0, len(buses))
	for _, bus := range buses {
		if err := provision(ctx, bus); err != nil {
			failures = append(failures, err)
		}
	}
	return errors.Join(failures...)
}

func provisionBus(ctx context.Context, bus provisioner.Bus) error {
	inbox := "_INBOX.PROVISIONER_" + strings.ToUpper(bus.Plane)
	nc, err := nats.Connect(bus.URL,
		nats.UserInfo(bus.User, bus.Password),
		nats.CustomInboxPrefix(inbox),
		nats.Timeout(5*time.Second),
		nats.Name("audit-observability-provisioner-"+bus.Name),
	)
	if err != nil {
		return fmt.Errorf("connect provisioner bus %q: %w", bus.Name, err)
	}
	defer nc.Close()
	js, err := nc.JetStream(nats.Context(ctx))
	if err != nil {
		return fmt.Errorf("open JetStream for %q: %w", bus.Name, err)
	}
	if err := provisioner.Provision(ctx, js, bus); err != nil {
		return fmt.Errorf("provision bus %q: %w", bus.Name, err)
	}
	return nil
}
