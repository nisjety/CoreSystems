package rbac

// catalog is the authoritative list of capability strings the UI can grant.
// Adding a new capability:
//   1. Add a CatalogEntry below.
//   2. Update any default-role seeds in migrations/002_add_enterprise_fields.up.sql
//      that should ship with the capability.
//   3. Update consumers that check the capability (auth-core middleware,
//      verevon guards, etc.).
//
// Capability key format: `<resource>:<action>` (lowercase, colon-separated).
// Groups are stable user-visible labels — the UI's role editor renders one
// section per group.
var catalog = []CatalogEntry{
	// Organisation
	{Key: "org:read", Group: "Organisation", Label: "Read organisation profile",
		Description: "View the workspace name, plan, and high-level settings."},
	{Key: "org:update", Group: "Organisation", Label: "Update organisation profile",
		Description: "Rename the workspace, change its tier, edit org metadata."},
	{Key: "org:delete", Group: "Organisation", Label: "Delete organisation",
		Description: "Permanently delete the workspace. Owners-only by default."},

	// Members
	{Key: "members:read", Group: "Members", Label: "View member list",
		Description: "See who belongs to the workspace and their roles."},
	{Key: "members:invite", Group: "Members", Label: "Invite members",
		Description: "Send invitations to new members."},
	{Key: "members:remove", Group: "Members", Label: "Remove members",
		Description: "Remove members from the workspace."},

	// Roles & permissions
	{Key: "roles:manage", Group: "Roles", Label: "Manage roles & permissions",
		Description: "Create or edit roles, assign capabilities to roles."},

	// Billing
	{Key: "billing:read", Group: "Billing", Label: "View invoices and plan",
		Description: "See invoices, plan tier, and seat counts."},
	{Key: "billing:manage", Group: "Billing", Label: "Manage billing",
		Description: "Update payment method, change plan, manage seats."},

	// Resources (workspace content — generic catch-all for the existing seed)
	{Key: "resources:create", Group: "Resources", Label: "Create resources",
		Description: "Create new documents, chats, agents, or other workspace content."},
	{Key: "resources:read", Group: "Resources", Label: "Read resources",
		Description: "View workspace content."},
	{Key: "resources:update", Group: "Resources", Label: "Update resources",
		Description: "Edit existing workspace content."},
	{Key: "resources:delete", Group: "Resources", Label: "Delete resources",
		Description: "Delete workspace content."},

	// Integrations
	{Key: "integrations:read", Group: "Integrations", Label: "View integrations",
		Description: "See connected sources (Microsoft 365, SharePoint, etc.)."},
	{Key: "integrations:manage", Group: "Integrations", Label: "Manage integrations",
		Description: "Connect or disconnect data sources."},

	// Support (Ticketing)
	{Key: "support:recurrence:read", Group: "Support", Label: "View support-recurrence similarity candidates",
		Description: "See semantic similarity candidates for a support ticket (preview)."},
}

// Catalog returns the immutable catalog.
func Catalog() []CatalogEntry {
	// Return a defensive copy so callers can't mutate the package var.
	out := make([]CatalogEntry, len(catalog))
	copy(out, catalog)
	return out
}

// CatalogKeys returns the set of valid capability keys for membership
// checks during create/update.
func CatalogKeys() map[string]struct{} {
	out := make(map[string]struct{}, len(catalog))
	for _, c := range catalog {
		out[c.Key] = struct{}{}
	}
	return out
}
