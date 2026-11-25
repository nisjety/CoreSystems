package spaces

import (
	"errors"
	"strings"
)

// WS10-S2: standing-rule provenance. A standing ("always") rule may only be
// built from an approval that exists, belongs to a run of the caller's Space,
// is visible to the caller (the requester, or the addressee the approval was
// routed to) and was granted. Session Core proves that through
// RunService.ResolveApprovalAuthority, a content-free projection that names no
// tool and no action: it proves provenance and nothing else, so it never widens
// a rule. The rule itself stays Control's (approval_grant.go).

var (
	// ErrApprovalAuthorityNotResolved: Session Core did not resolve the approval
	// (absent, outside a Space, under Zero Data Retention, service-owned, or an
	// incomplete projection). The caller sees approval_not_found.
	ErrApprovalAuthorityNotResolved = errors.New("approval authority not resolved")
	// ErrApprovalNotFound: the approval is not one the caller may build a rule
	// from. Another Space or organisation, and an approval the caller cannot see,
	// give the same refusal, so none of them can be probed.
	ErrApprovalNotFound = errors.New("approval not found")
	// ErrApprovalNotGranted: the approval is visible to the caller but was not
	// granted (requested, denied or timed out).
	ErrApprovalNotGranted = errors.New("approval is not granted")
)

// approvalStatusGranted is the approvals.status Session Core writes when a
// person grants an approval.
const approvalStatusGranted = "granted"

// approvalVisibleToCaller reports whether the approval is the caller's to see:
// the caller is the run's requester, or the approval's effective addressee (the
// same rule that lets a member decide it, approvalDecideAddresseeAllows).
func approvalVisibleToCaller(authority ApprovalDecideAuthority, subjectID, role string) bool {
	subjectID = strings.TrimSpace(subjectID)
	if subjectID != "" && subjectID == strings.TrimSpace(authority.RequesterSubjectID) {
		return true
	}
	return approvalDecideAddresseeAllows(authority, subjectID, role)
}

// VerifyStandingRuleProvenance proves that the approval a standing rule names is
// one the caller's current Space authority may build the rule from. evidence is
// the caller's freshly resolved manager authority and authority is Session
// Core's projection of the approval. The checks run in this order, so no answer
// tells the caller more than they may see: the projection must be for the
// approval asked about; it must belong to the caller's Space and organisation;
// it must be visible to the caller; and only then must it be granted.
func VerifyStandingRuleProvenance(evidence PersonalThreadDecisionEvidence, approvalID string, authority ApprovalDecideAuthority) error {
	approvalID = strings.TrimSpace(approvalID)
	if approvalID == "" || strings.TrimSpace(authority.ApprovalID) != approvalID ||
		strings.TrimSpace(authority.RunID) == "" || strings.TrimSpace(authority.State) == "" {
		return ErrApprovalAuthorityNotResolved
	}
	if authority.ZeroDataRetentionRun {
		return zdrForbidsDenial("approval_grant", "standing approval rules are not available under Zero Data Retention")
	}
	if strings.TrimSpace(authority.OrgID) != evidence.Membership.OrgID ||
		strings.TrimSpace(authority.SpaceRef) != evidence.Membership.SpaceRef {
		return ErrApprovalNotFound
	}
	if !approvalVisibleToCaller(authority, evidence.Membership.SubjectID, evidence.Membership.Role) {
		return ErrApprovalNotFound
	}
	if strings.TrimSpace(authority.State) != approvalStatusGranted {
		return ErrApprovalNotGranted
	}
	return nil
}
