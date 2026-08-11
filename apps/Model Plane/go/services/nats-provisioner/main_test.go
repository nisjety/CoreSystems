package main

import (
	"testing"
	"time"

	"github.com/nats-io/nats.go"
)

func TestTopologySpecsCoverRuntimeBindings(t *testing.T) {
	streams := streamConfigs()
	if len(streams) != 3 {
		t.Fatalf("stream count = %d, want 3", len(streams))
	}
	for _, stream := range streams {
		if stream.Name == "" || len(stream.Subjects) != 1 || stream.MaxAge == 0 {
			t.Fatalf("invalid stream spec: %+v", stream)
		}
		if stream.Storage != nats.FileStorage || stream.Replicas != 1 {
			t.Fatalf("runtime streams must be durable single-replica contracts: %+v", stream)
		}
	}
	bindings := consumerBindings()
	if len(bindings) != 2 {
		t.Fatalf("consumer count = %d, want 2", len(bindings))
	}
	if bindings[0].config.AckPolicy != nats.AckExplicitPolicy || bindings[0].config.AckWait != 30*time.Second {
		t.Fatalf("tool completion consumer must retry explicit work: %+v", bindings[0].config)
	}
	if bindings[1].config.AckPolicy != nats.AckNonePolicy {
		t.Fatalf("orchestration bridge must use at-most-once fan-out: %+v", bindings[1].config)
	}
}

func TestSameStringsIsOrderSensitive(t *testing.T) {
	if !sameStrings([]string{"a", "b"}, []string{"a", "b"}) {
		t.Fatal("equal subjects should match")
	}
	if sameStrings([]string{"a", "b"}, []string{"b", "a"}) {
		t.Fatal("subject order drift must be rejected")
	}
}
