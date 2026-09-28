// Package policy supplies additional domain checks over server-verified authority.
// It does NOT authenticate tokens, signatures, devices or HTTP requests.
package policy

import (
	"errors"
	"time"
)

type AssistanceAction string

const (
	View      AssistanceAction = "screen.view"
	Annotate  AssistanceAction = "screen.annotate"
	Pointer   AssistanceAction = "input.pointer"
	Keyboard  AssistanceAction = "input.keyboard"
	ObserveAI AssistanceAction = "screen.process-ai"
)

// VerifiedAccess is only to be assembled by a future trusted authority adapter,
// never decoded from the client body or trusted based on caller-supplied headers.
type VerifiedAccess struct {
	OrganizationID, GroupID, TeacherID, LearnerID, DeviceID, LocalSessionID string
	AuthorityVerified, DeviceAccepted                                       bool
}

type AssistanceGrant struct {
	Scope               VerifiedAccess
	IssuedAt, ExpiresAt time.Time
	Revoked             bool
	Permissions         []AssistanceAction
}

func CheckAssistance(access VerifiedAccess, grant AssistanceGrant, action AssistanceAction, now time.Time) error {
	if !access.AuthorityVerified || !grant.Scope.AuthorityVerified {
		return errors.New("authority not verified")
	}
	if access.OrganizationID == "" || access.GroupID == "" || access.TeacherID == "" || access.LearnerID == "" || access.DeviceID == "" || access.LocalSessionID == "" {
		return errors.New("incomplete assistance scope")
	}
	if access.OrganizationID != grant.Scope.OrganizationID || access.GroupID != grant.Scope.GroupID || access.TeacherID != grant.Scope.TeacherID || access.LearnerID != grant.Scope.LearnerID || access.DeviceID != grant.Scope.DeviceID || access.LocalSessionID != grant.Scope.LocalSessionID {
		return errors.New("assistance scope mismatch")
	}
	if !access.DeviceAccepted || !grant.Scope.DeviceAccepted {
		return errors.New("device has not accepted assistance")
	}
	if grant.Revoked || grant.IssuedAt.IsZero() || now.Before(grant.IssuedAt) || !now.Before(grant.ExpiresAt) || grant.ExpiresAt.Sub(grant.IssuedAt) > 15*time.Minute {
		return errors.New("grant revoked or outside bounded lifetime")
	}
	switch action {
	case View, Annotate, Pointer, Keyboard, ObserveAI:
	default:
		return errors.New("unsupported assistance action")
	}
	for _, permission := range grant.Permissions {
		if permission == action {
			return nil
		}
	}
	return errors.New("action not granted")
}

type Processor struct {
	Hosting                string
	InsideApprovedBoundary bool
	ExternalFallback       bool
}

// The processor must be resolved from trusted deployment policy, not user input.
func CheckStrictProcessing(processor Processor) error {
	if processor.Hosting != "self-hosted" || !processor.InsideApprovedBoundary || processor.ExternalFallback {
		return errors.New("processor is incompatible with strict-self-hosted policy")
	}
	return nil
}
