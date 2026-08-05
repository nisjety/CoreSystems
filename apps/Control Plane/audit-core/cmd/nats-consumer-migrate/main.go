// Command nats-consumer-migrate is a one-off operational tool for moving a
// single durable JetStream org-erasure consumer across a subject rename.
//
// ensureFixedConsumer in the provisioner deliberately refuses to mutate an
// existing consumer whose config differs from the wanted one — a mismatch is
// treated as someone else's intentional configuration, not "stale, safe to
// overwrite" (see provisioner.go's doc comment: it never deletes a stream,
// consumer, or message). That is the right default, but it also means a
// consumer whose FilterSubject changed because the SOURCE was renamed (not
// because an operator hand-edited it) never converges on its own.
//
// Running `nats-provisioner` cannot fix this either: ProvisionControlSharedRuntime
// stops at the FIRST resource that fails to converge, so an unrelated
// pre-existing mismatch earlier in its sequence blocks it from ever reaching a
// consumer that is otherwise ready to migrate.
//
// This tool converges exactly one consumer, independent of every other
// resource on the bus: if a drained (zero pending, zero ack-pending)
// mismatched consumer exists, it deletes it; either way, it then recreates
// the consumer from the CURRENT wanted config via
// provisioner.EnsureOrgErasureConsumer, so the desired shape of every
// consumer stays declared in exactly one place (provisioner.go).
//
// Usage:
//
//	NATS_URL=nats://control-shared-nats:4222 \
//	NATS_USER=control-shared-provisioner \
//	NATS_PASSWORD=... \
//	nats-consumer-migrate --stream=AQENCIA_CONTROLPLANE --durable=retrieval-engine-gdpr-erasure-v1
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log"
	"os"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/triodelab/controlplane/audit-core/internal/provisioner"
)

func main() {
	stream := flag.String("stream", "", "JetStream stream name")
	durable := flag.String("durable", "", "durable consumer name to migrate")
	flag.Parse()

	inboxPrefix := os.Getenv("NATS_INBOX_PREFIX")
	if inboxPrefix == "" {
		// Matches control-shared-provisioner's ACL'd subscribe permission
		// ("_INBOX.PROVISIONER_SHARED.>" in control-shared-nats.conf), the only
		// identity with $JS.API.STREAM.>/$JS.API.CONSUMER.> rights on
		// control-shared-nats today. A different bus/identity needs a matching
		// override via NATS_INBOX_PREFIX, not a code change here.
		inboxPrefix = "_INBOX.PROVISIONER_SHARED"
	}

	if err := run(*stream, *durable, os.Getenv("NATS_URL"), os.Getenv("NATS_USER"), os.Getenv("NATS_PASSWORD"), inboxPrefix); err != nil {
		log.Fatal(err)
	}
}

func run(stream, durable, url, user, password, inboxPrefix string) error {
	if stream == "" || durable == "" {
		return errors.New("both --stream and --durable are required")
	}
	if url == "" || user == "" || password == "" {
		return errors.New("NATS_URL, NATS_USER, and NATS_PASSWORD must all be set")
	}
	nc, err := nats.Connect(url,
		nats.UserInfo(user, password),
		nats.CustomInboxPrefix(inboxPrefix),
		nats.Timeout(5*time.Second),
		nats.Name("nats-consumer-migrate"),
	)
	if err != nil {
		return fmt.Errorf("connect: %w", err)
	}
	defer nc.Close()
	js, err := nc.JetStream()
	if err != nil {
		return fmt.Errorf("open JetStream: %w", err)
	}
	return migrate(context.Background(), js, stream, durable)
}

func migrate(ctx context.Context, js nats.JetStreamContext, stream, durable string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	info, err := js.ConsumerInfo(stream, durable)
	switch {
	case errors.Is(err, nats.ErrConsumerNotFound):
		log.Printf("consumer %s/%s does not exist yet; creating from current source", stream, durable)
	case err != nil:
		return fmt.Errorf("inspect consumer %s/%s: %w", stream, durable, err)
	default:
		if info.NumPending != 0 || info.NumAckPending != 0 {
			return fmt.Errorf(
				"refusing to migrate consumer %s/%s: %d pending, %d ack-pending message(s) would be discarded",
				stream, durable, info.NumPending, info.NumAckPending,
			)
		}
		if err := js.DeleteConsumer(stream, durable); err != nil {
			return fmt.Errorf("delete consumer %s/%s: %w", stream, durable, err)
		}
		log.Printf("deleted drained consumer %s/%s (was filter_subject=%q)", stream, durable, info.Config.FilterSubject)
	}
	if err := provisioner.EnsureOrgErasureConsumer(js, stream, durable); err != nil {
		return fmt.Errorf("recreate consumer %s/%s from current source: %w", stream, durable, err)
	}
	log.Printf("consumer %s/%s now converged to current source", stream, durable)
	return nil
}
