package nats

const (
	// User events
	SubjectUserCreated = "user.created"
	SubjectUserUpdated = "user.updated"
	SubjectUserDeleted = "user.deleted"

	// Organization events
	SubjectOrganizationCreated = "organization.created"
	SubjectOrganizationUpdated = "organization.updated"
	SubjectOrganizationDeleted = "organization.deleted"

	// Organization plan events
	SubjectOrganizationPlanChanged = "organization.plan.changed"

	// Billing-originated events consumed (not published) by org-core to
	// mirror billing-core's account state — see BillingSyncSubscriber.
	SubjectBillingAccountUpdated = "billing.account.updated"

	// Organization member events
	SubjectOrganizationMemberAdded   = "organization.member.added"
	SubjectOrganizationMemberRemoved = "organization.member.removed"
	SubjectOrganizationMemberUpdated = "organization.member.updated"

	// Session events
	SubjectSessionCreated = "session.created"
	SubjectSessionEnded   = "session.ended"
)

// Event payload structures matching Control Plane ownership

type UserCreatedEvent struct {
	Type      string                 `json:"type"`
	UserID    string                 `json:"user_id"`
	Email     string                 `json:"email"`
	Name      string                 `json:"name,omitempty"`
	Role      string                 `json:"role,omitempty"`
	Verified  bool                   `json:"verified"`
	Timestamp string                 `json:"timestamp"`
	TraceID   string                 `json:"trace_id,omitempty"`
	Metadata  map[string]interface{} `json:"metadata,omitempty"`
}

type UserUpdatedEvent struct {
	Type      string                 `json:"type"`
	UserID    string                 `json:"user_id"`
	Email     string                 `json:"email"`
	Changes   map[string]interface{} `json:"changes"`
	Timestamp string                 `json:"timestamp"`
	TraceID   string                 `json:"trace_id,omitempty"`
}

type OrganizationCreatedEvent struct {
	Type           string                 `json:"type"`
	OrganizationID string                 `json:"organization_id"`
	Name           string                 `json:"name"`
	Slug           string                 `json:"slug"`
	Plan           string                 `json:"plan"`
	CreatorID      string                 `json:"creator_id"`
	Timestamp      string                 `json:"timestamp"`
	TraceID        string                 `json:"trace_id,omitempty"`
	Metadata       map[string]interface{} `json:"metadata,omitempty"`
}

type OrganizationUpdatedEvent struct {
	Type           string                 `json:"type"`
	OrganizationID string                 `json:"organization_id"`
	Changes        map[string]interface{} `json:"changes"`
	UpdatedBy      string                 `json:"updated_by,omitempty"`
	Timestamp      string                 `json:"timestamp"`
	TraceID        string                 `json:"trace_id,omitempty"`
}

type OrganizationPlanChangedEvent struct {
	Type             string                 `json:"type"`
	OrganizationID   string                 `json:"organization_id"`
	OrganizationName string                 `json:"organization_name"`
	PreviousPlan     string                 `json:"previous_plan"`
	NewPlan          string                 `json:"new_plan"`
	ChangedBy        string                 `json:"changed_by,omitempty"`
	ChangeReason     string                 `json:"change_reason,omitempty"`
	Timestamp        string                 `json:"timestamp"`
	TraceID          string                 `json:"trace_id,omitempty"`
	Metadata         map[string]interface{} `json:"metadata,omitempty"`
}

type OrganizationMemberAddedEvent struct {
	Type             string `json:"type"`
	OrganizationID   string `json:"organization_id"`
	OrganizationName string `json:"organization_name"`
	UserID           string `json:"user_id"`
	UserEmail        string `json:"user_email"`
	Role             string `json:"role"`
	InvitedBy        string `json:"invited_by,omitempty"`
	Timestamp        string `json:"timestamp"`
	TraceID          string `json:"trace_id,omitempty"`
}
