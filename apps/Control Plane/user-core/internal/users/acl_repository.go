package users

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/authztaxonomy"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// resource_grants is the single authority for explicit per-subject access grants
// across every ownable resource type (see migration 012). Private/org visibility
// is carried by per-resource columns in the owning service (e.g. documents); this
// table only records the explicit shares that widen that default.

const (
	subjectTypeUser  = "user"
	subjectTypeSpace = "space"
	roleView         = "view"
	roleEdit         = "edit"

	resourceTypeDocument = "document"
)

// ResourceGrant is one explicit grant: subject S may access resource R in role.
type ResourceGrant struct {
	GrantID      string
	OrgID        string
	ResourceType string
	ResourceID   string
	SubjectType  string
	SubjectID    string
	Role         string
	GrantedBy    string
	GrantedAt    time.Time
}

// VisibleResources is the result of ListVisible: the explicit-grant resource ids
// a subject can see, plus an all-org sentinel for super-visibility (admin
// read_all). Private/org-visible resources are resolved by the owning service's
// columns, not enumerated here, so the id set stays small.
type VisibleResources struct {
	IDs    []string
	AllOrg bool
}

// AclRepository handles persistence of resource grants.
type AclRepository struct {
	db *database.DB
}

// NewAclRepository constructs an AclRepository using the shared database handle.
func NewAclRepository(db *database.DB) *AclRepository {
	return &AclRepository{db: db}
}

// ─── Generalized resource-grant API (the authz facade primitives) ───────────────

// Grant upserts an explicit grant. It fails closed for resource types that are
// not user-grantable (team-shared or unknown) and normalizes the role.
func (r *AclRepository) Grant(ctx context.Context, g *ResourceGrant) (*ResourceGrant, error) {
	if err := authztaxonomy.ValidateUserGrant(g.ResourceType); err != nil {
		return nil, fmt.Errorf("grant rejected: %w", err)
	}
	subjectType := g.SubjectType
	if subjectType == "" {
		subjectType = subjectTypeUser
	}
	if subjectType != subjectTypeUser {
		// MVP: only user subjects. Teams are a deferred subject_type.
		return nil, fmt.Errorf("grant rejected: subject_type %q not supported (MVP is user-only)", subjectType)
	}
	role, err := normalizeRole(g.Role)
	if err != nil {
		return nil, fmt.Errorf("grant rejected: %w", err)
	}
	grantID := g.GrantID
	if grantID == "" {
		grantID = uuid.New().String()
	}

	// Re-granting the same (subject, resource) is an idempotent upsert: the role
	// and granter are refreshed and granted_at is bumped to "now" so the share
	// list reflects the latest action. There is never more than one row per pair.
	row := r.db.Pool.QueryRow(ctx,
		`INSERT INTO resource_grants
		    (grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
		 ON CONFLICT (org_id, resource_type, resource_id, subject_type, subject_id)
		 DO UPDATE SET role = EXCLUDED.role, granted_by = EXCLUDED.granted_by, granted_at = NOW()
		 RETURNING grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by, granted_at`,
		grantID, g.OrgID, g.ResourceType, g.ResourceID, subjectType, g.SubjectID, role, g.GrantedBy,
	)
	out := &ResourceGrant{}
	if err := row.Scan(&out.GrantID, &out.OrgID, &out.ResourceType, &out.ResourceID,
		&out.SubjectType, &out.SubjectID, &out.Role, &out.GrantedBy, &out.GrantedAt); err != nil {
		return nil, fmt.Errorf("failed to upsert resource grant: %w", err)
	}
	return out, nil
}

// Revoke removes the grant for (org, resource, subject). Returns pgx.ErrNoRows
// (wrapped) when no such grant existed.
func (r *AclRepository) Revoke(ctx context.Context, orgID, resourceType, resourceID, subjectType, subjectID string) error {
	if subjectType == "" {
		subjectType = subjectTypeUser
	}
	result, err := r.db.Pool.Exec(ctx,
		`DELETE FROM resource_grants
		 WHERE org_id=$1 AND resource_type=$2 AND resource_id=$3 AND subject_type=$4 AND subject_id=$5`,
		orgID, resourceType, resourceID, subjectType, subjectID,
	)
	if err != nil {
		return fmt.Errorf("failed to revoke resource grant: %w", err)
	}
	if result.RowsAffected() == 0 {
		return fmt.Errorf("resource grant not found: %w", pgx.ErrNoRows)
	}
	return nil
}

// Check reports whether subject S holds a grant on resource R, and at which role.
func (r *AclRepository) Check(ctx context.Context, orgID, resourceType, resourceID, subjectType, subjectID string) (bool, string, error) {
	if subjectType == "" {
		subjectType = subjectTypeUser
	}
	var role string
	err := r.db.Pool.QueryRow(ctx,
		`SELECT role FROM resource_grants
		 WHERE org_id=$1 AND resource_type=$2 AND resource_id=$3 AND subject_type=$4 AND subject_id=$5`,
		orgID, resourceType, resourceID, subjectType, subjectID,
	).Scan(&role)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return false, "", nil
		}
		return false, "", fmt.Errorf("failed to check resource grant: %w", err)
	}
	return true, role, nil
}

// BatchCheck returns, for the given resource ids, the role each is granted to the
// subject. Ids without a grant are simply absent from the map. Empty input → empty map.
func (r *AclRepository) BatchCheck(ctx context.Context, orgID, resourceType, subjectType, subjectID string, resourceIDs []string) (map[string]string, error) {
	out := make(map[string]string, len(resourceIDs))
	if len(resourceIDs) == 0 {
		return out, nil
	}
	if subjectType == "" {
		subjectType = subjectTypeUser
	}
	rows, err := r.db.Pool.Query(ctx,
		`SELECT resource_id, role FROM resource_grants
		 WHERE org_id=$1 AND resource_type=$2 AND subject_type=$3 AND subject_id=$4
		   AND resource_id = ANY($5)`,
		orgID, resourceType, subjectType, subjectID, resourceIDs,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to batch-check resource grants: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var id, role string
		if err := rows.Scan(&id, &role); err != nil {
			return nil, fmt.Errorf("failed to scan batch-check row: %w", err)
		}
		out[id] = role
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("failed to iterate batch-check rows: %w", err)
	}
	return out, nil
}

// ListVisible returns the explicit-grant resource ids of a type that the subject
// can see in an org. Private/org-visible resources are NOT enumerated here (the
// owning service resolves those from its own visibility column), keeping this set
// bounded to actual shares. AllOrg is reserved for admin super-visibility (PR-5).
func (r *AclRepository) ListVisible(ctx context.Context, orgID, resourceType, subjectType, subjectID string) (*VisibleResources, error) {
	if subjectType == "" {
		subjectType = subjectTypeUser
	}
	rows, err := r.db.Pool.Query(ctx,
		`SELECT resource_id FROM resource_grants
		 WHERE org_id=$1 AND resource_type=$2 AND subject_type=$3 AND subject_id=$4
		 ORDER BY granted_at ASC`,
		orgID, resourceType, subjectType, subjectID,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to list visible resources: %w", err)
	}
	defer rows.Close()
	ids := make([]string, 0, 16)
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return nil, fmt.Errorf("failed to scan visible resource row: %w", err)
		}
		ids = append(ids, id)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("failed to iterate visible resource rows: %w", err)
	}
	return &VisibleResources{IDs: ids, AllOrg: false}, nil
}

// ListByResource returns all grants on a single resource (for the share dialog).
func (r *AclRepository) ListByResource(ctx context.Context, orgID, resourceType, resourceID string) ([]*ResourceGrant, error) {
	rows, err := r.db.Pool.Query(ctx,
		`SELECT grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by, granted_at
		 FROM resource_grants
		 WHERE org_id=$1 AND resource_type=$2 AND resource_id=$3
		 ORDER BY granted_at ASC`,
		orgID, resourceType, resourceID,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to list grants by resource: %w", err)
	}
	defer rows.Close()
	var grants []*ResourceGrant
	for rows.Next() {
		g := &ResourceGrant{}
		if err := rows.Scan(&g.GrantID, &g.OrgID, &g.ResourceType, &g.ResourceID,
			&g.SubjectType, &g.SubjectID, &g.Role, &g.GrantedBy, &g.GrantedAt); err != nil {
			return nil, fmt.Errorf("failed to scan grant row: %w", err)
		}
		grants = append(grants, g)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("failed to iterate grant rows: %w", err)
	}
	return grants, nil
}

// RevokeAllGrantsForUser removes every grant held BY a user across ALL orgs.
// Used by GDPR erasure: the user is gone, so every inbound share they held must
// disappear with them. Returns the number of grants revoked.
func (r *AclRepository) RevokeAllGrantsForUser(ctx context.Context, userID string) (int64, error) {
	result, err := r.db.Pool.Exec(ctx,
		`DELETE FROM resource_grants WHERE subject_type=$1 AND subject_id=$2`,
		subjectTypeUser, userID,
	)
	if err != nil {
		return 0, fmt.Errorf("failed to revoke all grants for user: %w", err)
	}
	return result.RowsAffected(), nil
}

// RevokeAllForSubject removes every grant held BY a subject in an org (GDPR
// erasure of inbound shares). Returns the number of grants revoked.
func (r *AclRepository) RevokeAllForSubject(ctx context.Context, orgID, subjectType, subjectID string) (int64, error) {
	if subjectType == "" {
		subjectType = subjectTypeUser
	}
	result, err := r.db.Pool.Exec(ctx,
		`DELETE FROM resource_grants WHERE org_id=$1 AND subject_type=$2 AND subject_id=$3`,
		orgID, subjectType, subjectID,
	)
	if err != nil {
		return 0, fmt.Errorf("failed to revoke grants for subject: %w", err)
	}
	return result.RowsAffected(), nil
}

// DeleteGrantsForOrg removes every resource grant of an organisation, across
// all resource types and subject types (this covers the shared-snapshot view
// grants a purged org leaves behind). It is the Control side of the cross-plane
// organisation erasure: Control owns grants, so user-core erases its own rows
// when the organisation is purged, and no other plane acts as the owner. It is
// a single statement and returns the number of rows removed; the caller logs
// the org id and that count only.
func (r *AclRepository) DeleteGrantsForOrg(ctx context.Context, orgID string) (int64, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return 0, fmt.Errorf("failed to delete grants for org: org id is required")
	}
	result, err := r.db.Pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id=$1`, orgID)
	if err != nil {
		return 0, fmt.Errorf("failed to delete grants for org: %w", err)
	}
	return result.RowsAffected(), nil
}

// NormalizeGrantRole is normalizeRole for callers outside the package (the
// owner-asserted facade decides role_not_supported from the normalized role).
func NormalizeGrantRole(role string) (string, error) { return normalizeRole(role) }

// IsActiveOrgMember reports whether the user is an active member of the org.
// The owner-asserted grant path refuses a subject outside the org: a share
// never crosses a tenant boundary.
func (r *AclRepository) IsActiveOrgMember(ctx context.Context, orgID, userID string) (bool, error) {
	var active bool
	if err := r.db.Pool.QueryRow(ctx,
		`SELECT EXISTS (SELECT 1 FROM user_org_memberships WHERE user_id=$1 AND org_id=$2 AND status='active')`,
		userID, orgID,
	).Scan(&active); err != nil {
		return false, fmt.Errorf("failed to check org membership: %w", err)
	}
	return active, nil
}

// ErrSpaceNotGrantable means the Space is not an active, shared Space of the
// grant's org, or the resource type cannot be granted to a Space.
var ErrSpaceNotGrantable = errors.New("space is not grantable for this resource")

// GrantSpaceSubject is the ONLY writer of a subject_type='space' row (WS14-S2).
// The row means "the owner attached this resource to the room": it is honoured
// only inside a Space read that also proves current membership and audience
// (ListVisibleForSpace), so it can narrow or co-require but never widen an
// owner ACL. It requires a Space-grantable resource type (documents only) and
// an active, non-personal Space registered in the grant's org. The role is
// always 'view'. Idempotent: replaying keeps the first granter and time and
// reports created=false.
func (r *AclRepository) GrantSpaceSubject(ctx context.Context, orgID, resourceType, resourceID, spaceRef, grantedBy string) (*ResourceGrant, bool, error) {
	if err := authztaxonomy.ValidateUserGrant(resourceType); err != nil {
		return nil, false, fmt.Errorf("grant rejected: %w", err)
	}
	if !authztaxonomy.SpaceGrantable(resourceType) {
		return nil, false, fmt.Errorf("grant rejected: %w", ErrSpaceNotGrantable)
	}
	var registered bool
	if err := r.db.Pool.QueryRow(ctx,
		`SELECT EXISTS (
		    SELECT 1 FROM registered_spaces
		    WHERE space_ref=$1 AND org_id=$2 AND registration_state='active' AND space_kind <> 'personal')`,
		spaceRef, orgID,
	).Scan(&registered); err != nil {
		return nil, false, fmt.Errorf("failed to resolve Space for grant: %w", err)
	}
	if !registered {
		return nil, false, fmt.Errorf("grant rejected: %w", ErrSpaceNotGrantable)
	}
	out := &ResourceGrant{}
	var created bool
	err := r.db.Pool.QueryRow(ctx,
		`INSERT INTO resource_grants
		    (grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by)
		 VALUES ($1, $2, $3, $4, 'space', $5, 'view', $6)
		 ON CONFLICT (org_id, resource_type, resource_id, subject_type, subject_id)
		 DO UPDATE SET role = resource_grants.role
		 RETURNING grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by, granted_at, (xmax = 0)`,
		uuid.New().String(), orgID, resourceType, resourceID, spaceRef, grantedBy,
	).Scan(&out.GrantID, &out.OrgID, &out.ResourceType, &out.ResourceID,
		&out.SubjectType, &out.SubjectID, &out.Role, &out.GrantedBy, &out.GrantedAt, &created)
	if err != nil {
		return nil, false, fmt.Errorf("failed to upsert Space grant: %w", err)
	}
	return out, created, nil
}

// ListVisibleForSpace returns the resource ids of a type that are attached to
// the Space (a subject_type='space' grant) AND that the user may see through
// the Space right now: the Space is registered in the org, not personal, and
// active or archived (read-only), the user is an active org member AND an
// active Space member AND in the Space's CURRENT recipient audience. ok=false
// (and no ids) means the user does not resolve as a current member; the caller
// treats that as an empty set, never as an error that leaks existence. The
// audience revision is the one the join used.
func (r *AclRepository) ListVisibleForSpace(ctx context.Context, orgID, resourceType, spaceRef, userID string) (ids []string, audienceRevision int64, ok bool, err error) {
	var granted []string
	err = r.db.Pool.QueryRow(ctx,
		`SELECT r.recipient_audience_revision,
		        ARRAY(SELECT g.resource_id FROM resource_grants g
		              WHERE g.org_id=s.org_id AND g.resource_type=$3
		                AND g.subject_type='space' AND g.subject_id=s.space_ref
		              ORDER BY g.granted_at ASC, g.resource_id ASC)
		 FROM registered_spaces s
		 JOIN space_memberships m ON m.space_ref=s.space_ref
		 JOIN space_authority_revisions r ON r.space_ref=s.space_ref
		 JOIN space_recipient_audience_members am
		   ON am.space_ref=s.space_ref AND am.revision=r.recipient_audience_revision AND am.subject_id=m.subject_id
		 WHERE s.space_ref=$1 AND s.org_id=$2
		   AND s.registration_state IN ('active','archived')
		   AND s.space_kind <> 'personal'
		   AND m.subject_type='user' AND m.subject_id=$4 AND m.active=TRUE
		   AND EXISTS (SELECT 1 FROM user_org_memberships u WHERE u.user_id=$4 AND u.org_id=$2 AND u.status='active')`,
		spaceRef, orgID, resourceType, userID,
	).Scan(&audienceRevision, &granted)
	if errors.Is(err, pgx.ErrNoRows) {
		return []string{}, 0, false, nil
	}
	if err != nil {
		return nil, 0, false, fmt.Errorf("failed to list Space-visible resources: %w", err)
	}
	if granted == nil {
		granted = []string{}
	}
	return granted, audienceRevision, true, nil
}

// RevokeSpaceGrants removes every grant whose subject is the Space, across
// resource types (the Space's attached files stop being reachable through it).
// Returns the number of rows removed. Space deletion does the same inside its
// own transaction (purgeControlSpaceProjections); this is the standalone form.
func (r *AclRepository) RevokeSpaceGrants(ctx context.Context, spaceRef string) (int64, error) {
	result, err := r.db.Pool.Exec(ctx,
		`DELETE FROM resource_grants WHERE subject_type='space' AND subject_id=$1`, spaceRef)
	if err != nil {
		return 0, fmt.Errorf("failed to revoke Space grants: %w", err)
	}
	return result.RowsAffected(), nil
}

// normalizeRole canonicalizes a caller role/permission string to 'view' or
// 'edit'. Known synonyms map through; an empty string is treated as the least
// privilege ('view') for back-compat; anything else is rejected so a typo can
// never silently corrupt a grant's privilege level.
func normalizeRole(role string) (string, error) {
	switch strings.ToLower(strings.TrimSpace(role)) {
	case "", "view", "read":
		return roleView, nil
	case "write", "edit", "admin", "owner":
		return roleEdit, nil
	default:
		return "", fmt.Errorf("unrecognised role %q (want one of: view/read/edit/write/admin/owner)", role)
	}
}

// ─── DocumentAcl back-compat adapters (DocumentAccessService gRPC) ───────────────
//
// The dormant DocumentAccessService still speaks in document/user/permission_level
// terms. These adapters map that surface onto resource_grants so there is exactly
// one storage table. permission_level is normalized to a role ('view'|'edit').

// DocumentAcl mirrors a single document grant for the gRPC handler.
type DocumentAcl struct {
	AclID           string
	OrgID           string
	DocumentID      string
	UserID          string
	PermissionLevel string
	CreatedAt       time.Time
}

func grantToDocumentAcl(g *ResourceGrant) *DocumentAcl {
	return &DocumentAcl{
		AclID:           g.GrantID,
		OrgID:           g.OrgID,
		DocumentID:      g.ResourceID,
		UserID:          g.SubjectID,
		PermissionLevel: g.Role,
		CreatedAt:       g.GrantedAt,
	}
}

// Create inserts a document grant (back-compat). GrantedBy is left empty: the
// dormant DocumentAccessService gRPC request carries no granting-actor identity.
// The live share path (HTTP authz facade, PR-6) populates granted_by from the
// authenticated caller.
func (r *AclRepository) Create(ctx context.Context, acl *DocumentAcl) error {
	_, err := r.Grant(ctx, &ResourceGrant{
		GrantID:      acl.AclID,
		OrgID:        acl.OrgID,
		ResourceType: resourceTypeDocument,
		ResourceID:   acl.DocumentID,
		SubjectType:  subjectTypeUser,
		SubjectID:    acl.UserID,
		Role:         acl.PermissionLevel,
	})
	return err
}

// Delete removes a grant by its primary key (back-compat).
func (r *AclRepository) Delete(ctx context.Context, grantID string) error {
	result, err := r.db.Pool.Exec(ctx, `DELETE FROM resource_grants WHERE grant_id = $1`, grantID)
	if err != nil {
		return fmt.Errorf("failed to delete resource grant: %w", err)
	}
	if result.RowsAffected() == 0 {
		return fmt.Errorf("resource grant not found: %w", pgx.ErrNoRows)
	}
	return nil
}

// scanDocumentAcl scans a single resource_grants row into a DocumentAcl,
// mapping pgx.ErrNoRows to a "document acl not found" error.
func scanDocumentAcl(row pgx.Row) (*DocumentAcl, error) {
	g := &ResourceGrant{}
	if err := row.Scan(&g.GrantID, &g.OrgID, &g.ResourceType, &g.ResourceID,
		&g.SubjectType, &g.SubjectID, &g.Role, &g.GrantedBy, &g.GrantedAt); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("document acl not found: %w", pgx.ErrNoRows)
		}
		return nil, fmt.Errorf("failed to get resource grant: %w", err)
	}
	return grantToDocumentAcl(g), nil
}

// GetByID retrieves a single grant by primary key (back-compat).
func (r *AclRepository) GetByID(ctx context.Context, grantID string) (*DocumentAcl, error) {
	row := r.db.Pool.QueryRow(ctx,
		`SELECT grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by, granted_at
		 FROM resource_grants WHERE grant_id = $1`, grantID)
	return scanDocumentAcl(row)
}

// ListByDocument returns all user grants on a document (back-compat).
func (r *AclRepository) ListByDocument(ctx context.Context, orgID, documentID string) ([]*DocumentAcl, error) {
	grants, err := r.ListByResource(ctx, orgID, resourceTypeDocument, documentID)
	if err != nil {
		return nil, err
	}
	entries := make([]*DocumentAcl, 0, len(grants))
	for _, g := range grants {
		entries = append(entries, grantToDocumentAcl(g))
	}
	return entries, nil
}

// HasAccess reports whether a user holds any grant on a document (back-compat).
func (r *AclRepository) HasAccess(ctx context.Context, orgID, documentID, userID string) (bool, error) {
	ok, _, err := r.Check(ctx, orgID, resourceTypeDocument, documentID, subjectTypeUser, userID)
	return ok, err
}

// DeleteByUser removes the document grant for (org, document, user) (back-compat).
func (r *AclRepository) DeleteByUser(ctx context.Context, orgID, documentID, userID string) error {
	return r.Revoke(ctx, orgID, resourceTypeDocument, documentID, subjectTypeUser, userID)
}

// GetByUser retrieves the document grant for (org, document, user) (back-compat).
func (r *AclRepository) GetByUser(ctx context.Context, orgID, documentID, userID string) (*DocumentAcl, error) {
	row := r.db.Pool.QueryRow(ctx,
		`SELECT grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by, granted_at
		 FROM resource_grants
		 WHERE org_id=$1 AND resource_type=$2 AND resource_id=$3 AND subject_type=$4 AND subject_id=$5`,
		orgID, resourceTypeDocument, documentID, subjectTypeUser, userID)
	return scanDocumentAcl(row)
}
