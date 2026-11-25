package http

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// wiringRunResolver stands in for a wired Session Core prepared-run read. The boot check reads
// only whether a resolver is set, so its answer is never used here.
type wiringRunResolver struct{}

func (wiringRunResolver) ResolveScheduledRunAuthority(context.Context, spaces.ScheduledRunIntent, string) (spaces.ScheduledRunAuthority, error) {
	return spaces.ScheduledRunAuthority{}, errors.New("the boot check never resolves a run")
}

// F-f10-27 rel1: a start that turns SCHEDULE_CONTRACT_ENABLED on is refused while no prepared-run
// authority is wired, because every run-execution decision would answer 503 and scheduled runs
// would stop. The refusal names the switch, so the operator can find it at boot.
func TestScheduleContractWiringRefusesTheSwitchWithoutAPreparedRunResolver(t *testing.T) {
	s := &Server{}
	s.SetScheduleContractsEnabled(true)
	err := s.ValidateScheduleContractWiring()
	if !errors.Is(err, ErrScheduleContractAuthorityUnwired) {
		t.Fatalf("switch on with no prepared-run resolver: err=%v, want ErrScheduleContractAuthorityUnwired", err)
	}
	if !strings.Contains(err.Error(), "SCHEDULE_CONTRACT_ENABLED") {
		t.Fatalf("the refusal does not name the switch: %v", err)
	}
}

// With the switch off the boot check never refuses, wired or not, so the default start is unchanged.
func TestScheduleContractWiringLeavesTheDefaultStartUnchanged(t *testing.T) {
	s := &Server{}
	s.SetScheduleContractsEnabled(false)
	if err := s.ValidateScheduleContractWiring(); err != nil {
		t.Fatalf("switch off with no resolver: err=%v, want nil", err)
	}
	s.SetScheduledRunAuthorityResolver(wiringRunResolver{})
	if err := s.ValidateScheduleContractWiring(); err != nil {
		t.Fatalf("switch off with a resolver: err=%v, want nil", err)
	}
}

// Once a prepared-run resolver is wired, the switch may start the server.
func TestScheduleContractWiringAcceptsTheSwitchOnceAResolverIsWired(t *testing.T) {
	s := &Server{}
	s.SetScheduleContractsEnabled(true)
	s.SetScheduledRunAuthorityResolver(wiringRunResolver{})
	if err := s.ValidateScheduleContractWiring(); err != nil {
		t.Fatalf("switch on with a resolver: err=%v, want nil", err)
	}
}
