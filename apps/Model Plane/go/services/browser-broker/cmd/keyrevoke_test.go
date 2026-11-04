package main

import (
	"testing"

	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	bbserver "github.com/triodelab/model-plane/services/browser-broker/internal/server"
)

func envMap(m map[string]string) func(string) string { return func(k string) string { return m[k] } }

func TestKeyRevokeGate_KeyGrantsNeedAConsumer(t *testing.T) {
	on := bbserver.DefaultConfig()
	on.KeyGrantsEnabled = true
	if err := keyRevokeGate(on, envMap(nil)); err == nil {
		t.Fatal("key grants on without a revoke NATS URL must refuse to start")
	}
	if err := keyRevokeGate(on, envMap(map[string]string{envKeyRevokeNatsURL: "  "})); err == nil {
		t.Fatal("a blank URL is no URL")
	}
	if err := keyRevokeGate(on, envMap(map[string]string{envKeyRevokeNatsURL: "nats://bus:4222"})); err != nil {
		t.Fatalf("configured: %v", err)
	}
	if err := keyRevokeGate(bbserver.DefaultConfig(), envMap(nil)); err != nil {
		t.Fatalf("switch off needs nothing: %v", err)
	}
}

func TestStartKeyRevokeConsumer_UnreachableBusDisablesKeyGrants(t *testing.T) {
	cfg := bbserver.DefaultConfig()
	cfg.KeyGrantsEnabled = true
	srv := bbserver.NewServerWithConfig(grant.NewMemoryRepository(), cfg)
	if !srv.KeyGrantsActive() {
		t.Fatal("precondition: key grants on")
	}
	stop := startKeyRevokeConsumer(cfg, envMap(map[string]string{envKeyRevokeNatsURL: "nats://127.0.0.1:1"}), srv)
	stop()
	if srv.KeyGrantsActive() {
		t.Fatal("a broker whose revoke consumer cannot connect must not issue key grants")
	}
	// Switch off: nothing is started and nothing changes.
	off := bbserver.NewServerWithConfig(grant.NewMemoryRepository(), bbserver.DefaultConfig())
	startKeyRevokeConsumer(bbserver.DefaultConfig(), envMap(nil), off)()
	if off.KeyGrantsActive() {
		t.Fatal("switch off must stay off")
	}
}
