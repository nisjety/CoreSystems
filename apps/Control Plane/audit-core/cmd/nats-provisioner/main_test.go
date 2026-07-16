package main

import (
	"context"
	"errors"
	"reflect"
	"testing"

	"github.com/triodelab/controlplane/audit-core/internal/provisioner"
)

func TestProvisioningAttemptsEveryBusWhenOneExtraPlaneIsUnavailable(t *testing.T) {
	buses := []provisioner.Bus{
		{Name: "model", Plane: "model"},
		{Name: "application", Plane: "application"},
	}
	var attempted []string
	err := runProvisioning(context.Background(), buses, func(_ context.Context, bus provisioner.Bus) error {
		attempted = append(attempted, bus.Name)
		if bus.Name == "model" {
			return errors.New("model unavailable")
		}
		return nil
	})
	if err == nil {
		t.Fatal("aggregate failure was not reported")
	}
	if !reflect.DeepEqual(attempted, []string{"model", "application"}) {
		t.Fatalf("attempted buses = %v", attempted)
	}
}

func TestOptionalBusFailureIsSkipped(t *testing.T) {
	// An optional external-plane bus that is down (e.g. application-nats absent)
	// must NOT fail provisioning — otherwise the container crash-loops under
	// `restart: on-failure`. The reachable required bus still provisions.
	buses := []provisioner.Bus{
		{Name: "control", Plane: "control"},
		{Name: "application", Plane: "application", Optional: true},
	}
	var attempted []string
	err := runProvisioning(context.Background(), buses, func(_ context.Context, bus provisioner.Bus) error {
		attempted = append(attempted, bus.Name)
		if bus.Name == "application" {
			return errors.New("application-nats unreachable")
		}
		return nil
	})
	if err != nil {
		t.Fatalf("optional bus failure must not fail provisioning: %v", err)
	}
	if !reflect.DeepEqual(attempted, []string{"control", "application"}) {
		t.Fatalf("attempted buses = %v", attempted)
	}
}

func TestRequiredBusFailureStillFails(t *testing.T) {
	// A failure on a NON-optional bus must still be reported (fatal) so a real
	// provisioning break is never silently swallowed.
	buses := []provisioner.Bus{
		{Name: "control", Plane: "control"},
		{Name: "application", Plane: "application", Optional: true},
	}
	err := runProvisioning(context.Background(), buses, func(_ context.Context, bus provisioner.Bus) error {
		if bus.Name == "control" {
			return errors.New("control-nats broken")
		}
		return nil
	})
	if err == nil {
		t.Fatal("a required-bus failure must be reported")
	}
}
