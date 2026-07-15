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
