package quarrycontracts

import "encoding/json"

// E3.1-a (ADR-2a): the Go mirror of `quarry_core::contracts::AgentAction::FillOtpRef`.
//
// Rust serialises AgentAction internally tagged (`{"type": "...", ...fields}`),
// so this variant is a flat struct rather than the generic AgentAction
// {Type, Payload} shape. There is no value, code or text field by design: the
// one-time code never travels on the wire. Quarry's executor reads it from
// the approved mail connection after the edge has admitted a single-use
// `fill_otp` approval. The edge refuses the action (403) unless
// QUARRY_EDGE__FILL_OTP is on (default off).

// AgentActionTypeFillOtpRef is the serde tag of AgentAction::FillOtpRef.
const AgentActionTypeFillOtpRef = "fill_otp_ref"

// SensitiveActionFillOtp is the BrowserBroker sensitive-action name an
// approval grant for a one-time-code fill must carry.
const SensitiveActionFillOtp = "fill_otp"

// FillOtpRefAction mirrors AgentAction::FillOtpRef field for field.
type FillOtpRefAction struct {
	Type            string `json:"type"`
	SnapshotID      string `json:"snapshot_id"`
	Generation      uint32 `json:"generation"`
	RefID           string `json:"ref_id"`
	ApprovalGrantID string `json:"approval_grant_id"`
}

// NewFillOtpRefAction builds the action for an exact target of the latest
// snapshot and a fresh single-use approval grant.
func NewFillOtpRefAction(snapshotID string, generation uint32, refID, approvalGrantID string) FillOtpRefAction {
	return FillOtpRefAction{
		Type:            AgentActionTypeFillOtpRef,
		SnapshotID:      snapshotID,
		Generation:      generation,
		RefID:           refID,
		ApprovalGrantID: approvalGrantID,
	}
}

// MarshalJSON pins the tag, so a zero-value Type never reaches the wire.
func (a FillOtpRefAction) MarshalJSON() ([]byte, error) {
	type wire FillOtpRefAction
	w := wire(a)
	w.Type = AgentActionTypeFillOtpRef
	return json.Marshal(w)
}
