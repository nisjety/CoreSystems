package http

import "errors"

// ErrScheduleContractAuthorityUnwired refuses a start that turns SCHEDULE_CONTRACT_ENABLED on
// while this build wires no Session Core prepared-run authority for run-execution decisions
// (F-f10-27 rel1). Without that read every well-formed run-execution decision answers 503, so
// scheduled runs stop; the refusal surfaces the misconfiguration at boot instead.
var ErrScheduleContractAuthorityUnwired = errors.New("SCHEDULE_CONTRACT_ENABLED is on, but no Session Core prepared-run authority is wired for run-execution decisions (F-f10-27 rel1); leave the switch off until it is")

// ValidateScheduleContractWiring is the boot check for the stored-consent wiring. With
// SCHEDULE_CONTRACT_ENABLED off it does nothing, so the default start is unchanged. With it on,
// it requires the prepared-run resolver that SetScheduledRunAuthorityResolver installs.
func (s *Server) ValidateScheduleContractWiring() error {
	if s.scheduleContractsEnabled && s.scheduledRunAuthority == nil {
		return ErrScheduleContractAuthorityUnwired
	}
	return nil
}
