// nats-namespace-migrate is a preflight by default; it never deletes broker data.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"os"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/triodelab/controlplane/audit-core/internal/provisioner"
)

func main() {
	apply := flag.Bool("apply", false, "apply the drained namespace cutover")
	paused := flag.Bool("publishers-paused", false, "confirm all publishers and consumers have been paused after draining")
	snapshot := flag.String("snapshot", "", "new private snapshot file, required for an existing-stream cutover")
	flag.Parse()
	if os.Getenv("NATS_URL") == "" || os.Getenv("NATS_USER") == "" || os.Getenv("NATS_PASSWORD") == "" {
		log.Fatal("NATS_URL, NATS_USER and NATS_PASSWORD are required")
	}
	inbox := os.Getenv("NATS_INBOX_PREFIX")
	if inbox == "" {
		inbox = "_INBOX.PROVISIONER_SHARED"
	}
	nc, err := nats.Connect(os.Getenv("NATS_URL"), nats.UserInfo(os.Getenv("NATS_USER"), os.Getenv("NATS_PASSWORD")), nats.CustomInboxPrefix(inbox), nats.Timeout(5*time.Second))
	if err != nil {
		log.Fatal("NATS connection failed")
	}
	defer nc.Close()
	js, err := nc.JetStream()
	if err != nil {
		log.Fatal("JetStream initialization failed")
	}
	save := func(value provisioner.NamespaceSnapshot) error {
		if *snapshot == "" {
			return fmt.Errorf("--snapshot is required")
		}
		data, err := json.MarshalIndent(value, "", "  ")
		if err != nil {
			return err
		}
		file, err := os.OpenFile(*snapshot, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if err != nil {
			return err
		}
		_, writeErr := file.Write(data)
		closeErr := file.Close()
		if writeErr != nil {
			return writeErr
		}
		return closeErr
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	if err := provisioner.MigrateControlSharedNamespace(ctx, js, *apply, *paused, save); err != nil {
		log.Fatal(err)
	}
	if *apply {
		log.Print("Control namespace cutover converged; historical stream and durable positions preserved")
	} else {
		log.Print("Namespace preflight passed; no broker resources changed")
	}
}
