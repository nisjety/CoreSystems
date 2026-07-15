package nats

import (
	"os"
	"strings"
	"testing"
)

func TestRuntimeNATSClientDoesNotOwnStreamOrConsumerTopology(t *testing.T) {
	for _, path := range []string{"client.go", "subscriber.go", "publisher.go"} {
		source, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		for _, forbidden := range []string{
			"CreateStream(", "CreateConsumer(", "ensureAuthStream(",
			"EnsureUserEventsStream(",
		} {
			if strings.Contains(string(source), forbidden) {
				t.Fatalf("runtime file %s retains topology mutation %q", path, forbidden)
			}
		}
	}
}
