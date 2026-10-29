package config

import (
	"fmt"
	"strings"
	"testing"
)

const (
	primaryKey   = "0123456789abcdef0123456789abcdef"
	secondaryKey = "fedcba9876543210fedcba9876543210"
)

// The secondary key is for rotation and for cmd/doffin-probe. The service never
// reads it, so it can neither be leaked from the service's memory by accident
// nor stop the service from starting.
func TestTheServiceNeverHoldsTheSecondaryKey(t *testing.T) {
	t.Parallel()

	cfg, err := LoadFrom(env(with(required(), map[string]string{
		"DOFFIN_API_KEY":           primaryKey,
		"DOFFIN_API_KEY_SECONDARY": secondaryKey,
	})))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if !cfg.Doffin.Configured() {
		t.Error("Doffin should be configured by the primary key")
	}
	if cfg.Doffin.SecondaryAPIKey.IsSet() {
		t.Error("the service loaded the secondary key; it must not")
	}
}

// A probe-only variable must never be able to stop the service starting, even
// when it is set in the wrong place.
func TestASecondaryKeyDoesNotStopTheServiceStarting(t *testing.T) {
	t.Parallel()

	if _, err := LoadFrom(env(with(required(), map[string]string{"DOFFIN_API_KEY_SECONDARY": secondaryKey}))); err != nil {
		t.Fatalf("a secondary key alone must not fail the service's config: %v", err)
	}
}

func TestLoadDoffinFromReadsAndTrimsTheSecondaryKey(t *testing.T) {
	t.Parallel()

	d, err := LoadDoffinFrom(env(map[string]string{
		"DOFFIN_API_KEY":           primaryKey,
		"DOFFIN_API_KEY_SECONDARY": " " + secondaryKey + "\n",
	}))
	if err != nil {
		t.Fatalf("LoadDoffinFrom: %v", err)
	}
	if got := d.SecondaryAPIKey.Reveal(); got != secondaryKey {
		t.Errorf("secondary key not read and trimmed: %q", got)
	}
	if got := d.APIKey.Reveal(); got != primaryKey {
		t.Errorf("primary key = %q", got)
	}
}

func TestTheSecondaryKeyIsOptionalForTheProbe(t *testing.T) {
	t.Parallel()

	d, err := LoadDoffinFrom(env(map[string]string{"DOFFIN_API_KEY": primaryKey}))
	if err != nil {
		t.Fatalf("LoadDoffinFrom: %v", err)
	}
	if d.SecondaryAPIKey.IsSet() {
		t.Error("the secondary key should be unset when not supplied")
	}
}

// For the probe, a secondary key on its own is almost certainly the two lines
// the wrong way round.
func TestTheProbeRejectsASecondaryKeyWithoutAPrimary(t *testing.T) {
	t.Parallel()

	_, err := LoadDoffinFrom(env(map[string]string{"DOFFIN_API_KEY_SECONDARY": secondaryKey}))
	if err == nil {
		t.Fatal("expected an error")
	}
	for _, name := range []string{"DOFFIN_API_KEY_SECONDARY", "DOFFIN_API_KEY"} {
		if !strings.Contains(err.Error(), name) {
			t.Errorf("the error should name %s: %q", name, err)
		}
	}
	if strings.Contains(err.Error(), secondaryKey) {
		t.Errorf("the error echoed the key: %q", err)
	}
}

func TestDoffinSettingsNeverRenderEitherKey(t *testing.T) {
	t.Parallel()

	d, err := LoadDoffinFrom(env(map[string]string{
		"DOFFIN_API_KEY":           primaryKey,
		"DOFFIN_API_KEY_SECONDARY": secondaryKey,
	}))
	if err != nil {
		t.Fatalf("LoadDoffinFrom: %v", err)
	}
	rendered := fmt.Sprintf("%v | %+v | %#v", d, d, d)
	for _, key := range []string{primaryKey, secondaryKey} {
		if strings.Contains(rendered, key) {
			t.Fatalf("rendering leaked a key: %s", rendered)
		}
	}
}
