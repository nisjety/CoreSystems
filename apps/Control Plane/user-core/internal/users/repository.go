package users

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/jackc/pgx/v5"
)

// Repository handles user data persistence
type Repository struct {
	db *database.DB
}

// NewRepository creates a new user repository
func NewRepository(db *database.DB) *Repository {
	return &Repository{db: db}
}

// Create creates a new user
func (r *Repository) Create(ctx context.Context, params CreateUserParams, passwordHash string) (*User, error) {
	query := `
		INSERT INTO users (email, name, password_hash, avatar, status, email_verified, onboarding_complete)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
		RETURNING id, email, name, password_hash, avatar, status, email_verified, onboarding_complete, created_at, updated_at, last_login_at
	`

	user := &User{}
	err := r.db.Pool.QueryRow(ctx, query,
		params.Email,
		params.Name,
		passwordHash,
		params.Avatar,
		UserStatusActive,
		false,
		false, // onboarding_complete
	).Scan(
		&user.ID,
		&user.Email,
		&user.Name,
		&user.PasswordHash,
		&user.Avatar,
		&user.Status,
		&user.EmailVerified,
		&user.OnboardingComplete,
		&user.CreatedAt,
		&user.UpdatedAt,
		&user.LastLoginAt,
	)

	if err != nil {
		return nil, fmt.Errorf("failed to create user: %w", err)
	}

	return user, nil
}

// CreateWithID creates a new user with a specific ID (for OAuth users)
func (r *Repository) CreateWithID(ctx context.Context, id string, params CreateUserParams) (*User, error) {
	query := `
		INSERT INTO users (id, email, name, password_hash, avatar, status, email_verified, onboarding_complete)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
		RETURNING id, email, name, password_hash, avatar, status, email_verified, onboarding_complete, created_at, updated_at, last_login_at
	`

	user := &User{}
	err := r.db.Pool.QueryRow(ctx, query,
		id,
		params.Email,
		params.Name,
		"", // empty password hash for OAuth users
		params.Avatar,
		UserStatusActive,
		false, // email_verified
		false, // onboarding_complete - OAuth users need onboarding
	).Scan(
		&user.ID,
		&user.Email,
		&user.Name,
		&user.PasswordHash,
		&user.Avatar,
		&user.Status,
		&user.EmailVerified,
		&user.OnboardingComplete,
		&user.CreatedAt,
		&user.UpdatedAt,
		&user.LastLoginAt,
	)

	if err != nil {
		return nil, fmt.Errorf("failed to create user with ID: %w", err)
	}

	return user, nil
}

// GetByID retrieves a user by ID
func (r *Repository) GetByID(ctx context.Context, id string) (*User, error) {
	query := `
		SELECT id, email, name, password_hash, avatar, status, email_verified, onboarding_complete, created_at, updated_at, last_login_at
		FROM users
		WHERE id = $1
	`

	user := &User{}
	err := r.db.Pool.QueryRow(ctx, query, id).Scan(
		&user.ID,
		&user.Email,
		&user.Name,
		&user.PasswordHash,
		&user.Avatar,
		&user.Status,
		&user.EmailVerified,
		&user.OnboardingComplete,
		&user.CreatedAt,
		&user.UpdatedAt,
		&user.LastLoginAt,
	)

	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("user not found")
		}
		return nil, fmt.Errorf("failed to get user: %w", err)
	}

	return user, nil
}

// GetByEmail retrieves a user by email
func (r *Repository) GetByEmail(ctx context.Context, email string) (*User, error) {
	query := `
		SELECT id, email, name, password_hash, avatar, status, email_verified, onboarding_complete, created_at, updated_at, last_login_at
		FROM users
		WHERE email = $1
	`

	user := &User{}
	err := r.db.Pool.QueryRow(ctx, query, email).Scan(
		&user.ID,
		&user.Email,
		&user.Name,
		&user.PasswordHash,
		&user.Avatar,
		&user.Status,
		&user.EmailVerified,
		&user.OnboardingComplete,
		&user.CreatedAt,
		&user.UpdatedAt,
		&user.LastLoginAt,
	)

	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("user not found")
		}
		return nil, fmt.Errorf("failed to get user: %w", err)
	}

	return user, nil
}

// ReassignID changes a user-core row to the canonical auth-service user ID.
// It is used during local cutovers when a stale user-core row exists for the
// same email under an older generated ID.
func (r *Repository) ReassignID(ctx context.Context, currentID, nextID string) (*User, error) {
	query := `
		UPDATE users
		SET id = $2,
		    updated_at = NOW()
		WHERE id = $1
		RETURNING id, email, name, password_hash, avatar, status, email_verified, onboarding_complete, created_at, updated_at, last_login_at
	`

	user := &User{}
	err := r.db.Pool.QueryRow(ctx, query, currentID, nextID).Scan(
		&user.ID,
		&user.Email,
		&user.Name,
		&user.PasswordHash,
		&user.Avatar,
		&user.Status,
		&user.EmailVerified,
		&user.OnboardingComplete,
		&user.CreatedAt,
		&user.UpdatedAt,
		&user.LastLoginAt,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("user not found")
		}
		return nil, fmt.Errorf("failed to reassign user ID: %w", err)
	}

	return user, nil
}

// Update updates a user
func (r *Repository) Update(ctx context.Context, params UpdateUserParams) (*User, error) {
	query := `
		UPDATE users
		SET name = COALESCE($2, name),
		    avatar = COALESCE($3, avatar),
		    email = COALESCE($4, email),
		    updated_at = NOW()
		WHERE id = $1
		RETURNING id, email, name, password_hash, avatar, status, email_verified, onboarding_complete, created_at, updated_at, last_login_at
	`

	user := &User{}
	err := r.db.Pool.QueryRow(ctx, query,
		params.ID,
		params.Name,
		params.Avatar,
		params.Email,
	).Scan(
		&user.ID,
		&user.Email,
		&user.Name,
		&user.PasswordHash,
		&user.Avatar,
		&user.Status,
		&user.EmailVerified,
		&user.OnboardingComplete,
		&user.CreatedAt,
		&user.UpdatedAt,
		&user.LastLoginAt,
	)

	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("user not found")
		}
		return nil, fmt.Errorf("failed to update user: %w", err)
	}

	return user, nil
}

// Delete deletes a user
func (r *Repository) Delete(ctx context.Context, id string) error {
	query := `DELETE FROM users WHERE id = $1`

	result, err := r.db.Pool.Exec(ctx, query, id)
	if err != nil {
		return fmt.Errorf("failed to delete user: %w", err)
	}

	if result.RowsAffected() == 0 {
		return fmt.Errorf("user not found")
	}

	return nil
}

// List lists users with pagination
func (r *Repository) List(ctx context.Context, params ListUsersParams) ([]*User, int, error) {
	offset := (params.Page - 1) * params.Limit

	// Build query
	query := `
		SELECT id, email, name, password_hash, avatar, status, email_verified, onboarding_complete, created_at, updated_at, last_login_at
		FROM users
	`
	countQuery := `SELECT COUNT(*) FROM users`
	args := []interface{}{}
	argPos := 1

	if params.Status != nil {
		query += fmt.Sprintf(" WHERE status = $%d", argPos)
		countQuery += fmt.Sprintf(" WHERE status = $%d", argPos)
		args = append(args, string(*params.Status))
		argPos++
	}

	query += fmt.Sprintf(" ORDER BY created_at DESC LIMIT $%d OFFSET $%d", argPos, argPos+1)
	args = append(args, params.Limit, offset)

	// Get total count
	var total int
	countArgs := args[:len(args)-2] // Exclude limit and offset
	err := r.db.Pool.QueryRow(ctx, countQuery, countArgs...).Scan(&total)
	if err != nil {
		return nil, 0, fmt.Errorf("failed to count users: %w", err)
	}

	// Get users
	rows, err := r.db.Pool.Query(ctx, query, args...)
	if err != nil {
		return nil, 0, fmt.Errorf("failed to list users: %w", err)
	}
	defer rows.Close()

	users := []*User{}
	for rows.Next() {
		user := &User{}
		err := rows.Scan(
			&user.ID,
			&user.Email,
			&user.Name,
			&user.PasswordHash,
			&user.Avatar,
			&user.Status,
			&user.EmailVerified,
			&user.OnboardingComplete,
			&user.CreatedAt,
			&user.UpdatedAt,
			&user.LastLoginAt,
		)
		if err != nil {
			return nil, 0, fmt.Errorf("failed to scan user: %w", err)
		}
		users = append(users, user)
	}

	if err := rows.Err(); err != nil {
		return nil, 0, fmt.Errorf("error iterating users: %w", err)
	}

	return users, total, nil
}

// UpdateStatus updates a user's status
func (r *Repository) UpdateStatus(ctx context.Context, id string, status UserStatus) (*User, error) {
	query := `
		UPDATE users
		SET status = $2, updated_at = NOW()
		WHERE id = $1
		RETURNING id, email, name, password_hash, avatar, status, email_verified, created_at, updated_at, last_login_at
	`

	user := &User{}
	err := r.db.Pool.QueryRow(ctx, query, id, string(status)).Scan(
		&user.ID,
		&user.Email,
		&user.Name,
		&user.PasswordHash,
		&user.Avatar,
		&user.Status,
		&user.EmailVerified,
		&user.CreatedAt,
		&user.UpdatedAt,
		&user.LastLoginAt,
	)

	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("user not found")
		}
		return nil, fmt.Errorf("failed to update user status: %w", err)
	}

	return user, nil
}

// UpdateLastLogin updates a user's last login timestamp
func (r *Repository) UpdateLastLogin(ctx context.Context, id string) error {
	query := `UPDATE users SET last_login_at = $2 WHERE id = $1`

	_, err := r.db.Pool.Exec(ctx, query, id, time.Now())
	if err != nil {
		return fmt.Errorf("failed to update last login: %w", err)
	}

	return nil
}

// GetProfile retrieves a user profile
func (r *Repository) GetProfile(ctx context.Context, userID string) (*UserProfile, error) {
	query := `
		SELECT user_id, bio, phone, location, timezone, language, metadata, updated_at
		FROM user_profiles
		WHERE user_id = $1
	`

	profile := &UserProfile{}
	err := r.db.Pool.QueryRow(ctx, query, userID).Scan(
		&profile.UserID,
		&profile.Bio,
		&profile.Phone,
		&profile.Location,
		&profile.Timezone,
		&profile.Language,
		&profile.Metadata,
		&profile.UpdatedAt,
	)

	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("profile not found")
		}
		return nil, fmt.Errorf("failed to get profile: %w", err)
	}

	return profile, nil
}

// UpdateProfile upserts a user profile using INSERT ... ON CONFLICT.
func (r *Repository) UpdateProfile(ctx context.Context, params UpdateProfileParams) (*UserProfile, error) {
	query := `
		INSERT INTO user_profiles (user_id, bio, phone, location, timezone, language, metadata)
		VALUES ($1, COALESCE($2, ''), COALESCE($3, ''), COALESCE($4, ''), COALESCE($5, ''), COALESCE($6, ''), $7)
		ON CONFLICT (user_id) DO UPDATE SET
			bio = COALESCE($2, user_profiles.bio),
			phone = COALESCE($3, user_profiles.phone),
			location = COALESCE($4, user_profiles.location),
			timezone = COALESCE($5, user_profiles.timezone),
			language = COALESCE($6, user_profiles.language),
			metadata = COALESCE($7, user_profiles.metadata),
			updated_at = NOW()
		RETURNING user_id, bio, phone, location, timezone, language, metadata, updated_at
	`

	profile := &UserProfile{}
	err := r.db.Pool.QueryRow(ctx, query,
		params.UserID,
		params.Bio,
		params.Phone,
		params.Location,
		params.Timezone,
		params.Language,
		params.Metadata,
	).Scan(
		&profile.UserID,
		&profile.Bio,
		&profile.Phone,
		&profile.Location,
		&profile.Timezone,
		&profile.Language,
		&profile.Metadata,
		&profile.UpdatedAt,
	)

	if err != nil {
		return nil, fmt.Errorf("failed to upsert profile: %w", err)
	}

	return profile, nil
}

// ============================================
// USER SETTINGS REPOSITORY
// ============================================

// GetSettings retrieves a single settings row by user + category.
// Returns nil, nil when the row doesn't exist (caller should use defaults).
func (r *Repository) GetSettings(ctx context.Context, userID, category string) (*UserSettings, error) {
	query := `
		SELECT id, user_id, category, settings, created_at, updated_at
		FROM user_settings
		WHERE user_id = $1 AND category = $2
	`
	us := &UserSettings{}
	var raw []byte
	err := r.db.Pool.QueryRow(ctx, query, userID, category).Scan(
		&us.ID, &us.UserID, &us.Category, &raw, &us.CreatedAt, &us.UpdatedAt,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, nil
		}
		return nil, fmt.Errorf("failed to get settings (%s): %w", category, err)
	}
	if err := json.Unmarshal(raw, &us.Settings); err != nil {
		return nil, fmt.Errorf("failed to unmarshal settings (%s): %w", category, err)
	}
	return us, nil
}

// UpsertSettings inserts or merges settings for a user + category using JSONB || operator.
// Partial updates are safe: only the supplied keys are overwritten.
func (r *Repository) UpsertSettings(ctx context.Context, params UpsertSettingsParams) (*UserSettings, error) {
	id, err := generateUserSettingsID()
	if err != nil {
		return nil, fmt.Errorf("failed to generate user settings id: %w", err)
	}

	raw, err := json.Marshal(params.Settings)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal settings: %w", err)
	}

	query := `
		INSERT INTO user_settings (id, user_id, category, settings)
		VALUES ($1, $2, $3, $4::jsonb)
		ON CONFLICT (user_id, category) DO UPDATE SET
			settings   = user_settings.settings || EXCLUDED.settings,
			updated_at = NOW()
		RETURNING id, user_id, category, settings, created_at, updated_at
	`
	us := &UserSettings{}
	var returned []byte
	err = r.db.Pool.QueryRow(ctx, query, id, params.UserID, params.Category, raw).Scan(
		&us.ID, &us.UserID, &us.Category, &returned, &us.CreatedAt, &us.UpdatedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to upsert settings (%s): %w", params.Category, err)
	}
	if err := json.Unmarshal(returned, &us.Settings); err != nil {
		return nil, fmt.Errorf("failed to unmarshal upserted settings (%s): %w", params.Category, err)
	}
	return us, nil
}

func generateUserSettingsID() (string, error) {
	bytes := make([]byte, 16)
	if _, err := rand.Read(bytes); err != nil {
		return "", err
	}
	return "uset_" + hex.EncodeToString(bytes), nil
}

// ============================================
// PROVIDER ACCOUNTS REPOSITORY
// ============================================

// UpsertProviderAccount inserts or updates an OAuth provider account link.
// Called from the NATS event handler whenever a social sign-in succeeds.
func (r *Repository) UpsertProviderAccount(ctx context.Context, params UpsertProviderAccountParams) (*ProviderAccount, error) {
	id, err := generateProviderAccountID()
	if err != nil {
		return nil, fmt.Errorf("failed to generate provider account id: %w", err)
	}

	meta, err := json.Marshal(params.Metadata)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal provider metadata: %w", err)
	}
	scopes, err := json.Marshal(params.ScopesGranted)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal scopes: %w", err)
	}

	query := `
		INSERT INTO provider_accounts (
			id, user_id, provider, provider_user_id, tenant_id, microsoft_tenant_id,
			email, email_from_provider, display_name, scopes_granted, token_ref, metadata, last_synced_at
		)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb, $11, $12::jsonb, NOW())
		ON CONFLICT (provider, provider_user_id) DO UPDATE SET
			user_id      = EXCLUDED.user_id,
			tenant_id    = COALESCE(EXCLUDED.tenant_id, provider_accounts.tenant_id),
			microsoft_tenant_id = COALESCE(EXCLUDED.microsoft_tenant_id, provider_accounts.microsoft_tenant_id),
			email        = COALESCE(EXCLUDED.email, provider_accounts.email),
			email_from_provider = COALESCE(EXCLUDED.email_from_provider, provider_accounts.email_from_provider),
			display_name = COALESCE(EXCLUDED.display_name, provider_accounts.display_name),
			scopes_granted = CASE
				WHEN EXCLUDED.scopes_granted = '[]'::jsonb THEN provider_accounts.scopes_granted
				ELSE EXCLUDED.scopes_granted
			END,
			token_ref = COALESCE(EXCLUDED.token_ref, provider_accounts.token_ref),
			metadata     = provider_accounts.metadata || EXCLUDED.metadata,
			last_synced_at = NOW(),
			updated_at   = NOW()
		RETURNING id, user_id, provider, provider_user_id, tenant_id, microsoft_tenant_id, email, email_from_provider, display_name, scopes_granted, token_ref, metadata, created_at, updated_at, last_synced_at
	`
	pa := &ProviderAccount{}
	var retMeta []byte
	var retScopes []byte
	err = r.db.Pool.QueryRow(ctx, query,
		id, params.UserID, params.Provider, params.ProviderUserID,
		params.TenantID, params.MicrosoftTenantID, params.Email, params.EmailFromProvider, params.DisplayName, scopes, params.TokenRef, meta,
	).Scan(
		&pa.ID, &pa.UserID, &pa.Provider, &pa.ProviderUserID,
		&pa.TenantID, &pa.MicrosoftTenantID, &pa.Email, &pa.EmailFromProvider, &pa.DisplayName, &retScopes, &pa.TokenRef, &retMeta,
		&pa.CreatedAt, &pa.UpdatedAt, &pa.LastSyncedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to upsert provider account: %w", err)
	}
	if retScopes != nil {
		_ = json.Unmarshal(retScopes, &pa.ScopesGranted)
	}
	if retMeta != nil {
		if err := json.Unmarshal(retMeta, &pa.Metadata); err != nil {
			return nil, fmt.Errorf("failed to unmarshal provider metadata: %w", err)
		}
	}
	return pa, nil
}

func generateProviderAccountID() (string, error) {
	bytes := make([]byte, 16)
	if _, err := rand.Read(bytes); err != nil {
		return "", err
	}
	return "pa_" + hex.EncodeToString(bytes), nil
}

// GetProviderAccounts returns all linked provider accounts for a user.
func (r *Repository) GetProviderAccounts(ctx context.Context, userID string) ([]*ProviderAccount, error) {
	query := `
		SELECT id, user_id, provider, provider_user_id, tenant_id, microsoft_tenant_id, email, email_from_provider, display_name, scopes_granted, token_ref, metadata, created_at, updated_at, last_synced_at
		FROM provider_accounts
		WHERE user_id = $1
		ORDER BY created_at ASC
	`
	rows, err := r.db.Pool.Query(ctx, query, userID)
	if err != nil {
		return nil, fmt.Errorf("failed to list provider accounts: %w", err)
	}
	defer rows.Close()

	var accounts []*ProviderAccount
	for rows.Next() {
		pa := &ProviderAccount{}
		var meta []byte
		var scopes []byte
		if err := rows.Scan(
			&pa.ID, &pa.UserID, &pa.Provider, &pa.ProviderUserID,
			&pa.TenantID, &pa.MicrosoftTenantID, &pa.Email, &pa.EmailFromProvider, &pa.DisplayName, &scopes, &pa.TokenRef, &meta,
			&pa.CreatedAt, &pa.UpdatedAt, &pa.LastSyncedAt,
		); err != nil {
			return nil, fmt.Errorf("failed to scan provider account: %w", err)
		}
		if scopes != nil {
			_ = json.Unmarshal(scopes, &pa.ScopesGranted)
		}
		if meta != nil {
			_ = json.Unmarshal(meta, &pa.Metadata)
		}
		accounts = append(accounts, pa)
	}
	return accounts, rows.Err()
}

// EnsureUserOrgMembership upserts a user-organization membership.
func (r *Repository) EnsureUserOrgMembership(ctx context.Context, params EnsureMembershipParams) (*UserOrgMembership, error) {
	query := `
		INSERT INTO user_org_memberships (user_id, org_id, role, status)
		VALUES ($1, $2, $3, $4)
		ON CONFLICT (user_id, org_id) DO UPDATE SET
			role = EXCLUDED.role,
			status = EXCLUDED.status,
			updated_at = NOW()
		RETURNING id, user_id, org_id, role, status, COALESCE(invited_by, ''), created_at, updated_at
	`

	out := &UserOrgMembership{}
	err := r.db.Pool.QueryRow(ctx, query, params.UserID, params.OrgID, params.Role, params.Status).Scan(
		&out.ID,
		&out.UserID,
		&out.OrgID,
		&out.Role,
		&out.Status,
		&out.InvitedBy,
		&out.CreatedAt,
		&out.UpdatedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to ensure user org membership: %w", err)
	}

	return out, nil
}

func (r *Repository) RemoveUserOrgMembership(ctx context.Context, userID, orgID string) error {
	query := `
		UPDATE user_org_memberships
		SET status = 'removed', updated_at = NOW()
		WHERE user_id = $1 AND org_id = $2
	`

	if _, err := r.db.Pool.Exec(ctx, query, userID, orgID); err != nil {
		return fmt.Errorf("failed to remove user org membership: %w", err)
	}

	return nil
}

// GetPrimaryUserOrgMembership returns the most relevant active membership for a user.
func (r *Repository) GetPrimaryUserOrgMembership(ctx context.Context, userID string) (*UserOrgMembership, error) {
	query := `
		SELECT id, user_id, org_id, role, status, COALESCE(invited_by, ''), created_at, updated_at
		FROM user_org_memberships
		WHERE user_id = $1 AND status = 'active'
		ORDER BY
			CASE role
				WHEN 'owner' THEN 0
				WHEN 'admin' THEN 1
				WHEN 'member' THEN 2
				ELSE 3
			END,
			created_at ASC
		LIMIT 1
	`

	out := &UserOrgMembership{}
	err := r.db.Pool.QueryRow(ctx, query, userID).Scan(
		&out.ID,
		&out.UserID,
		&out.OrgID,
		&out.Role,
		&out.Status,
		&out.InvitedBy,
		&out.CreatedAt,
		&out.UpdatedAt,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, nil
		}
		return nil, fmt.Errorf("failed to get primary membership: %w", err)
	}

	return out, nil
}

// GetUserOrgMembership returns the user's active membership for a specific org,
// or nil when the user is not an active member of that org. Used to scope the
// session context to the org the user is currently acting as (the session's
// active organization) rather than always the primary membership.
func (r *Repository) GetUserOrgMembership(ctx context.Context, userID, orgID string) (*UserOrgMembership, error) {
	query := `
		SELECT id, user_id, org_id, role, status, COALESCE(invited_by, ''), created_at, updated_at
		FROM user_org_memberships
		WHERE user_id = $1 AND org_id = $2 AND status = 'active'
		LIMIT 1
	`

	out := &UserOrgMembership{}
	err := r.db.Pool.QueryRow(ctx, query, userID, orgID).Scan(
		&out.ID,
		&out.UserID,
		&out.OrgID,
		&out.Role,
		&out.Status,
		&out.InvitedBy,
		&out.CreatedAt,
		&out.UpdatedAt,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, nil
		}
		return nil, fmt.Errorf("failed to get user org membership: %w", err)
	}

	return out, nil
}

// CountActiveOrgMemberships returns active membership count for role bootstrap rules.
func (r *Repository) CountActiveOrgMemberships(ctx context.Context, orgID string) (int, error) {
	query := `SELECT COUNT(*) FROM user_org_memberships WHERE org_id = $1 AND status = 'active'`
	var count int
	if err := r.db.Pool.QueryRow(ctx, query, orgID).Scan(&count); err != nil {
		return 0, fmt.Errorf("failed to count org memberships: %w", err)
	}
	return count, nil
}

// GetUserByProviderID looks up a local user via their provider identity.
// Used by auth-core to resolve which local user a social login belongs to.
func (r *Repository) GetUserByProviderID(ctx context.Context, provider, providerUserID string) (*User, error) {
	query := `
		SELECT u.id, u.email, u.name, u.password_hash, u.avatar,
		       u.status, u.email_verified, u.created_at, u.updated_at, u.last_login_at
		FROM users u
		JOIN provider_accounts pa ON pa.user_id = u.id
		WHERE pa.provider = $1 AND pa.provider_user_id = $2
		LIMIT 1
	`
	user := &User{}
	err := r.db.Pool.QueryRow(ctx, query, provider, providerUserID).Scan(
		&user.ID, &user.Email, &user.Name, &user.PasswordHash, &user.Avatar,
		&user.Status, &user.EmailVerified, &user.CreatedAt, &user.UpdatedAt, &user.LastLoginAt,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("user not found for provider %s / %s", provider, providerUserID)
		}
		return nil, fmt.Errorf("failed to get user by provider ID: %w", err)
	}
	return user, nil
}

// MarkOnboardingComplete marks a user's onboarding as complete
func (r *Repository) MarkOnboardingComplete(ctx context.Context, email string) error {
	query := `UPDATE users SET onboarding_complete = true, updated_at = NOW() WHERE email = $1`

	result, err := r.db.Pool.Exec(ctx, query, email)
	if err != nil {
		return fmt.Errorf("failed to mark onboarding complete: %w", err)
	}

	if result.RowsAffected() == 0 {
		return fmt.Errorf("user not found with email: %s", email)
	}

	return nil
}

// MarkOnboardingCompleteByID marks a user's onboarding as complete by user ID
func (r *Repository) MarkOnboardingCompleteByID(ctx context.Context, userID string) error {
	query := `UPDATE users SET onboarding_complete = true, updated_at = NOW() WHERE id = $1`

	result, err := r.db.Pool.Exec(ctx, query, userID)
	if err != nil {
		return fmt.Errorf("failed to mark onboarding complete: %w", err)
	}

	if result.RowsAffected() == 0 {
		return fmt.Errorf("user not found with id: %s", userID)
	}

	return nil
}

// ============================================
// ONBOARDING STATE (G3 + G16)
// ============================================

// GetOnboardingState reads the user's persisted wizard step + opaque state
// blob. Returns ("", nil, nil) when the user has no in-flight state.
// Callers should treat both fields as advisory client cache (the canonical
// "done?" flag remains `users.onboarding_complete`).
func (r *Repository) GetOnboardingState(ctx context.Context, userID string) (string, []byte, error) {
	query := `SELECT COALESCE(onboarding_step, ''), onboarding_state FROM users WHERE id = $1`
	var step string
	var state []byte
	err := r.db.Pool.QueryRow(ctx, query, userID).Scan(&step, &state)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return "", nil, fmt.Errorf("user not found with id: %s", userID)
		}
		return "", nil, fmt.Errorf("failed to read onboarding state: %w", err)
	}
	return step, state, nil
}

// UpsertOnboardingState writes both columns atomically. A nil `state` clears
// the JSONB column; an empty `step` clears the step column. The user must
// exist — this is an UPDATE, not an INSERT (users come from auth-core via
// the provisioning path, not this endpoint).
func (r *Repository) UpsertOnboardingState(ctx context.Context, userID, step string, state []byte) error {
	query := `
		UPDATE users
		SET onboarding_step  = NULLIF($2, ''),
		    onboarding_state = $3::JSONB,
		    updated_at       = NOW()
		WHERE id = $1`
	result, err := r.db.Pool.Exec(ctx, query, userID, step, state)
	if err != nil {
		return fmt.Errorf("failed to upsert onboarding state: %w", err)
	}
	if result.RowsAffected() == 0 {
		return fmt.Errorf("user not found with id: %s", userID)
	}
	return nil
}

// ============================================
// ACTIVITY LOG
// ============================================

// LogActivity inserts an activity log entry for a user
func (r *Repository) LogActivity(ctx context.Context, params LogActivityParams) (*ActivityLog, error) {
	detailsJSON, err := json.Marshal(params.Details)
	if err != nil {
		detailsJSON = []byte("{}")
	}

	query := `
		INSERT INTO user_activity_log (user_id, action, resource, details, ip_address, user_agent)
		VALUES ($1, $2, $3, $4, $5, $6)
		RETURNING id, user_id, action, resource, details, ip_address, user_agent, created_at
	`

	var activity ActivityLog
	var detailsRaw []byte
	err = r.db.Pool.QueryRow(ctx, query,
		params.UserID, params.Action, params.Resource,
		detailsJSON, params.IPAddress, params.UserAgent,
	).Scan(
		&activity.ID, &activity.UserID, &activity.Action, &activity.Resource,
		&detailsRaw, &activity.IPAddress, &activity.UserAgent, &activity.CreatedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to log activity: %w", err)
	}

	if detailsRaw != nil {
		_ = json.Unmarshal(detailsRaw, &activity.Details)
	}

	return &activity, nil
}

// ListActivities returns paginated activity log entries for a user
func (r *Repository) ListActivities(ctx context.Context, userID string, page, limit int) ([]*ActivityLog, int, error) {
	if page < 1 {
		page = 1
	}
	if limit <= 0 {
		limit = 20
	}
	offset := (page - 1) * limit

	var total int
	if err := r.db.Pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM user_activity_log WHERE user_id = $1`, userID,
	).Scan(&total); err != nil {
		return nil, 0, fmt.Errorf("failed to count activities: %w", err)
	}

	rows, err := r.db.Pool.Query(ctx, `
		SELECT id, user_id, action, resource, details, ip_address, user_agent, created_at
		FROM user_activity_log
		WHERE user_id = $1
		ORDER BY created_at DESC
		LIMIT $2 OFFSET $3
	`, userID, limit, offset)
	if err != nil {
		return nil, 0, fmt.Errorf("failed to list activities: %w", err)
	}
	defer rows.Close()

	var activities []*ActivityLog
	for rows.Next() {
		var act ActivityLog
		var detailsRaw []byte
		if err := rows.Scan(
			&act.ID, &act.UserID, &act.Action, &act.Resource,
			&detailsRaw, &act.IPAddress, &act.UserAgent, &act.CreatedAt,
		); err != nil {
			return nil, 0, fmt.Errorf("failed to scan activity: %w", err)
		}
		if detailsRaw != nil {
			_ = json.Unmarshal(detailsRaw, &act.Details)
		}
		activities = append(activities, &act)
	}

	return activities, total, nil
}

// ============================================
// API KEY REPOSITORY
// ============================================

// CreateAPIKey inserts a new API key row and returns it (without the hash).
func (r *Repository) CreateAPIKey(ctx context.Context, userID, name, description, keyHash, keyPrefix string, scopes []string, expiresAt *time.Time) (*APIKey, error) {
	query := `
		INSERT INTO user_api_keys (user_id, name, description, key_hash, key_prefix, scopes, expires_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
		RETURNING id, user_id, name, description, key_prefix, scopes, expires_at, revoked_at, last_used_at, created_at
	`
	if scopes == nil {
		scopes = []string{}
	}
	k := &APIKey{}
	err := r.db.Pool.QueryRow(ctx, query,
		userID, name, description, keyHash, keyPrefix, scopes, expiresAt,
	).Scan(
		&k.ID, &k.UserID, &k.Name, &k.Description, &k.KeyPrefix,
		&k.Scopes, &k.ExpiresAt, &k.RevokedAt, &k.LastUsedAt, &k.CreatedAt,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to create API key: %w", err)
	}
	return k, nil
}

// ListAPIKeys returns all API keys for a user ordered by creation date.
func (r *Repository) ListAPIKeys(ctx context.Context, userID string) ([]*APIKey, error) {
	rows, err := r.db.Pool.Query(ctx, `
		SELECT id, user_id, name, description, key_prefix, scopes, expires_at, revoked_at, last_used_at, created_at
		FROM user_api_keys
		WHERE user_id = $1
		ORDER BY created_at DESC
	`, userID)
	if err != nil {
		return nil, fmt.Errorf("failed to list API keys: %w", err)
	}
	defer rows.Close()

	var keys []*APIKey
	for rows.Next() {
		k := &APIKey{}
		if err := rows.Scan(
			&k.ID, &k.UserID, &k.Name, &k.Description, &k.KeyPrefix,
			&k.Scopes, &k.ExpiresAt, &k.RevokedAt, &k.LastUsedAt, &k.CreatedAt,
		); err != nil {
			return nil, fmt.Errorf("failed to scan API key: %w", err)
		}
		keys = append(keys, k)
	}
	return keys, nil
}

// RevokeAPIKey sets revoked_at on an API key owned by the given user.
func (r *Repository) RevokeAPIKey(ctx context.Context, userID, keyID string) error {
	res, err := r.db.Pool.Exec(ctx,
		`UPDATE user_api_keys SET revoked_at = NOW() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL`,
		keyID, userID,
	)
	if err != nil {
		return fmt.Errorf("failed to revoke API key: %w", err)
	}
	if res.RowsAffected() == 0 {
		return fmt.Errorf("API key not found or already revoked")
	}
	return nil
}
