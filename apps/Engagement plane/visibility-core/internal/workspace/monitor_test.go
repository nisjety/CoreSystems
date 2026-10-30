package workspace

import (
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
)

func TestMonitoringProjectionOwnerIsImmutable(t *testing.T) {
	original := New(pulse.Config{}, nil)
	monitor := siteaudit.NewMonitor(nil)
	updated := original.WithMonitoring(monitor)
	if original.Monitoring() != nil || updated.Monitoring() != monitor || updated == original {
		t.Fatal("monitoring wiring mutated workspace")
	}
}
