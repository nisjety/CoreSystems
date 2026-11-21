package main

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadowner"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
	"google.golang.org/grpc"
)

func TestThreadOwnerCheckEnabledRequiresTheStrictLiteral(t *testing.T) {
	for _, value := range []string{"", "false", "TRUE", "True", "1", "yes", "on", " true", "true "} {
		if threadOwnerCheckEnabled(value) {
			t.Fatalf("%q must not enable the thread owner check", value)
		}
	}
	if !threadOwnerCheckEnabled("true") {
		t.Fatal(`the literal "true" must enable the thread owner check`)
	}
}

func envOf(values map[string]string) func(string) string {
	return func(key string) string { return values[key] }
}

// TestThreadOwnerFromEnvNeedsTheServiceCredential: the switch on with no credential
// to mint a session-core token is an error startup stops on, and the error names the
// variable to set and no value.
func TestThreadOwnerFromEnvNeedsTheServiceCredential(t *testing.T) {
	wiring, err := threadOwnerFromEnv(envOf(map[string]string{}))
	if err == nil || wiring != nil {
		t.Fatalf("threadOwnerFromEnv with no credential = (%v, %v), want an error", wiring, err)
	}
	if !strings.Contains(err.Error(), "SANDBOX_MANAGER_SERVICE_API_KEY") {
		t.Fatalf("error = %q, want it to name SANDBOX_MANAGER_SERVICE_API_KEY", err)
	}
}

func TestThreadOwnerFromEnvBuildsTheCachedResolver(t *testing.T) {
	wiring, err := threadOwnerFromEnv(envOf(map[string]string{
		"SANDBOX_MANAGER_SERVICE_API_KEY": "fixture-key-aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
	}))
	if err != nil {
		t.Fatalf("threadOwnerFromEnv: %v", err)
	}
	defer func() { _ = wiring.conn.Close() }()
	if _, ok := wiring.resolver.(*threadowner.Cache); !ok {
		t.Fatalf("resolver is %T, want the positive cache over session-core", wiring.resolver)
	}
	if wiring.tokens == nil {
		t.Fatal("no token source for the orphan sweep to reuse")
	}
}

// fakeTransport is an ExistenceTransport that is never called.
type fakeTransport struct{}

func (fakeTransport) ThreadsExist(context.Context, string, string, []string) (map[string]bool, error) {
	return nil, nil
}

// TestOrphanSweepForLeavesTheSweeperAloneWithoutATransport: no existence read is
// built, so the sweeper is returned as it was and no pass is configured.
func TestOrphanSweepForLeavesTheSweeperAloneWithoutATransport(t *testing.T) {
	previous := newExistenceTransport
	newExistenceTransport = nil
	defer func() { newExistenceTransport = previous }()

	sweeper := threadworkspace.NewSweeper(nil, nil, time.Hour)
	got, err := orphanSweepFor(sweeper, nil, &threadOwnerWiring{}, envOf(nil))
	if err != nil || got != sweeper {
		t.Fatalf("orphanSweepFor = (%p, %v), want the same sweeper and no error", got, err)
	}
}

func TestOrphanSweepForRefusesAMalformedGraceWindow(t *testing.T) {
	for _, bad := range []string{"0", "-1", "tomorrow"} {
		_, err := orphanSweepFor(threadworkspace.NewSweeper(nil, nil, time.Hour), nil, &threadOwnerWiring{},
			envOf(map[string]string{"SANDBOX_MANAGER_THREAD_ORPHAN_GRACE_HOURS": bad}))
		if err == nil {
			t.Fatalf("grace %q was accepted", bad)
		}
	}
}

func TestOrphanSweepForConfiguresThePassWhenATransportExists(t *testing.T) {
	previous := newExistenceTransport
	newExistenceTransport = func(grpc.ClientConnInterface) threadowner.ExistenceTransport { return fakeTransport{} }
	defer func() { newExistenceTransport = previous }()

	sweeper := threadworkspace.NewSweeper(nil, nil, time.Hour)
	got, err := orphanSweepFor(sweeper, nil, &threadOwnerWiring{tokens: tokenStub{}}, envOf(nil))
	if err != nil {
		t.Fatalf("orphanSweepFor: %v", err)
	}
	if got == sweeper {
		t.Fatal("the sweeper was not copied with the orphan pass configured")
	}
}

type tokenStub struct{}

func (tokenStub) Token(context.Context, string) (string, error) { return "fixture-token", nil }
