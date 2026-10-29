package events

import "fmt"

// Subjects defines the canonical NATS subject names for finspo events.
// All subjects are prefixed with the configured NATSSubjectPrefix (default "finspo").
type Subjects struct {
	prefix string
}

func NewSubjects(prefix string) Subjects {
	if prefix == "" {
		prefix = "finspo"
	}
	return Subjects{prefix: prefix}
}

func (s Subjects) ItemUpserted() string { return fmt.Sprintf("%s.item.upserted", s.prefix) }
func (s Subjects) ItemDeleted() string  { return fmt.Sprintf("%s.item.deleted", s.prefix) }
func (s Subjects) SourceSynced() string { return fmt.Sprintf("%s.source.synced", s.prefix) }
func (s Subjects) AuditEmitted() string { return fmt.Sprintf("%s.audit.emitted", s.prefix) }

// Proposal lifecycle subjects (Phase 5).
func (s Subjects) ProposalCreated() string  { return fmt.Sprintf("%s.proposal.created", s.prefix) }
func (s Subjects) ProposalApproved() string { return fmt.Sprintf("%s.proposal.approved", s.prefix) }
func (s Subjects) ProposalRejected() string { return fmt.Sprintf("%s.proposal.rejected", s.prefix) }
func (s Subjects) ProposalExecuted() string { return fmt.Sprintf("%s.proposal.executed", s.prefix) }
func (s Subjects) ProposalFailed() string   { return fmt.Sprintf("%s.proposal.failed", s.prefix) }

// ForProposalStatus maps a proposal status string to its lifecycle subject.
// Returns "" for statuses that have no dedicated subject (e.g. pending).
func (s Subjects) ForProposalStatus(status string) string {
	switch status {
	case "approved":
		return s.ProposalApproved()
	case "rejected":
		return s.ProposalRejected()
	case "executed":
		return s.ProposalExecuted()
	case "failed":
		return s.ProposalFailed()
	default:
		return ""
	}
}
