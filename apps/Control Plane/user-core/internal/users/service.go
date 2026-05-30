package users

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/redis"
	"golang.org/x/crypto/bcrypt"
)

// truncateRunes trims a string to at most max runes.
func truncateRunes(s string, max int) string {
	if max <= 0 {
		return ""
	}
	r := []rune(s)
	if len(r) <= max {
		return s
	}
	return string(r[:max])
}

const (
	userCacheTTL    = 5 * time.Minute
	userIDKeyPrefix = "user:id:"
)

// SharedPublisher is satisfied by *nats.SharedPublisher.
// Defined here (not importing nats) to avoid import cycles.
type SharedPublisher interface {
	PublishUserRegistered(ctx context.Context, userID, email, name, provider string)
	PublishUserUpdated(ctx context.Context, userID, email string, changes map[string]any)
	PublishUserDeleted(ctx context.Context, userID, email string)
	PublishProviderLinked(ctx context.Context, userID, email, provider, tenantID string)
	PublishProviderReadyForIntegration(
		ctx context.Context,
		userID string,
		email string,
		provider string,
		providerAccountID string,
		tenantID string,
		microsoftTenantID string,
		scopesGranted []string,
		tokenRef string,
		orgID string,
		role string,
		onboardingStatus string,
	)
}

// Service handles user business logic
type Service struct {
	repo             *Repository
	bcryptCost       int
	betterAuthClient interface{}        // Better Auth client (optional, can be nil)
	eventPublisher   interface{}        // NATS publisher (optional, can be nil)
	sharedPublisher  SharedPublisher    // cross-plane events on velion-nats
	cache            *rediscache.Client // optional, nil if Redis disabled
}

// NewService creates a new user service
// betterAuthClient, eventPublisher, and cache are optional (can be nil)
func NewService(repo *Repository, betterAuthClient interface{}, eventPublisher interface{}, cache ...*rediscache.Client) *Service {
	svc := &Service{
		repo:             repo,
		bcryptCost:       bcrypt.DefaultCost,
		betterAuthClient: betterAuthClient,
		eventPublisher:   eventPublisher,
	}
	if len(cache) > 0 {
		svc.cache = cache[0]
	}
	return svc
}

// SetSharedPublisher wires the cross-plane NATS publisher for controlplane.user.* subjects.
func (s *Service) SetSharedPublisher(sp SharedPublisher) {
	s.sharedPublisher = sp
}

// CreateUser creates a new user with hashed password
func (s *Service) CreateUser(ctx context.Context, params CreateUserParams) (*User, error) {
	// Validate email
	if params.Email == "" {
		return nil, fmt.Errorf("email is required")
	}

	// Validate name
	if params.Name == "" {
		return nil, fmt.Errorf("name is required")
	}

	// Check if user already exists
	existing, err := s.repo.GetByEmail(ctx, params.Email)
	if err == nil && existing != nil {
		return nil, fmt.Errorf("user with email %s already exists", params.Email)
	}

	// Hash password
	passwordHash := ""
	if params.Password != "" {
		hash, err := bcrypt.GenerateFromPassword([]byte(params.Password), s.bcryptCost)
		if err != nil {
			return nil, fmt.Errorf("failed to hash password: %w", err)
		}
		passwordHash = string(hash)
	}

	// Create user
	user, err := s.repo.Create(ctx, params, passwordHash)
	if err != nil {
		return nil, err
	}

	// Publish registration event
	s.publishUserRegistered(ctx, user.ID, user.Email, user.Name, "local")

	return user, nil
}

// GetUser retrieves a user by ID
func (s *Service) GetUser(ctx context.Context, id string) (*User, error) {
	if id == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	// Cache-aside: try Redis first
	if s.cache != nil {
		key := userIDKeyPrefix + id
		if cached, err := s.cache.Get(ctx, key); err == nil {
			var u User
			if json.Unmarshal([]byte(cached), &u) == nil {
				return &u, nil
			}
		}
	}

	user, err := s.repo.GetByID(ctx, id)
	if err != nil {
		return nil, err
	}

	// Populate cache on miss
	if s.cache != nil {
		if b, merr := json.Marshal(user); merr == nil {
			_ = s.cache.Set(ctx, userIDKeyPrefix+id, string(b), userCacheTTL)
		}
	}

	return user, nil
}

// GetOrCreateUser retrieves a user by ID, creating a minimal profile if it doesn't exist
// This is used for auto-provisioning users from OAuth/Better Auth
func (s *Service) GetOrCreateUser(ctx context.Context, id, email, name, avatar string) (*User, error) {
	if id == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	// Sanitize incoming fields to match DB column sizes:
	// email: 255, name: 255, avatar: 500
	if email != "" {
		email = truncateRunes(email, 255)
	}
	if name != "" {
		name = truncateRunes(name, 255)
	}
	if avatar != "" {
		avatar = truncateRunes(avatar, 500)
	}

	// Use cache-aware GetUser for the initial lookup
	user, err := s.GetUser(ctx, id)
	if err == nil {
		updateParams := UpdateUserParams{ID: id}
		needsUpdate := false

		if email != "" {
			hasPlaceholderEmail := strings.HasSuffix(user.Email, "@placeholder.local") || strings.TrimSpace(user.Email) == ""
			if hasPlaceholderEmail && user.Email != email {
				updateParams.Email = &email
				needsUpdate = true
			}
		}

		if name != "" {
			hasPlaceholderName := strings.TrimSpace(user.Name) == "" || user.Name == "User"
			if hasPlaceholderName && user.Name != name {
				updateParams.Name = &name
				needsUpdate = true
			}
		}

		if avatar != "" {
			hasPlaceholderAvatar := strings.TrimSpace(user.Avatar) == "" ||
				strings.Contains(strings.ToLower(user.Avatar), "example.com/avatar") ||
				strings.Contains(strings.ToLower(user.Avatar), "placeholder")
			if hasPlaceholderAvatar && user.Avatar != avatar {
				updateParams.Avatar = &avatar
				needsUpdate = true
			}
		}

		if needsUpdate {
			updatedUser, updateErr := s.repo.Update(ctx, updateParams)
			if updateErr == nil {
				// Invalidate stale cache entries
				if s.cache != nil {
					_ = s.cache.Del(ctx, userIDKeyPrefix+id)
				}
				return updatedUser, nil
			}
		}

		return user, nil
	}

	// User doesn't exist, create minimal profile
	if email == "" {
		email = fmt.Sprintf("user-%s@placeholder.local", id)
	}
	if name == "" {
		name = "User"
	}

	params := CreateUserParams{
		Email:  email,
		Name:   name,
		Avatar: avatar,
	}

	created, err := s.repo.CreateWithID(ctx, id, params)
	if err != nil {
		return nil, err
	}

	// Populate cache for newly created user
	if s.cache != nil {
		if b, merr := json.Marshal(created); merr == nil {
			_ = s.cache.Set(ctx, userIDKeyPrefix+created.ID, string(b), userCacheTTL)
		}
	}

	// Publish registration event if this is a new user
	if created.ID == id && (email != "" || name != "") {
		s.publishUserRegistered(ctx, created.ID, created.Email, created.Name, "oauth")
	}

	return created, nil
}

// GetUserByEmail retrieves a user by email
func (s *Service) GetUserByEmail(ctx context.Context, email string) (*User, error) {
	if email == "" {
		return nil, fmt.Errorf("email is required")
	}

	// Cache-aside: look up by email (stores user under id key after lookup)
	if s.cache != nil {
		emailKey := "user:email:" + email
		if idVal, err := s.cache.Get(ctx, emailKey); err == nil {
			// idVal is the user ID — try to fetch from id cache
			if cached, err2 := s.cache.Get(ctx, userIDKeyPrefix+idVal); err2 == nil {
				var u User
				if json.Unmarshal([]byte(cached), &u) == nil {
					return &u, nil
				}
			}
		}
	}

	user, err := s.repo.GetByEmail(ctx, email)
	if err != nil {
		return nil, err
	}

	// Populate cache on miss
	if s.cache != nil {
		if b, merr := json.Marshal(user); merr == nil {
			_ = s.cache.Set(ctx, userIDKeyPrefix+user.ID, string(b), userCacheTTL)
			_ = s.cache.Set(ctx, "user:email:"+user.Email, user.ID, userCacheTTL)
		}
	}

	return user, nil
}

// UpdateUser updates a user
func (s *Service) UpdateUser(ctx context.Context, params UpdateUserParams) (*User, error) {
	if params.ID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	// Check if the new email is already taken by another user
	// (skip the separate GetByID — Update returns ErrNoRows if user doesn't exist)
	if params.Email != nil && *params.Email != "" {
		existing, err := s.repo.GetByEmail(ctx, *params.Email)
		if err == nil && existing != nil && existing.ID != params.ID {
			return nil, fmt.Errorf("email %s is already taken", *params.Email)
		}
	}

	updated, err := s.repo.Update(ctx, params)
	if err != nil {
		return nil, err
	}

	// Invalidate cached entries for this user
	if s.cache != nil {
		_ = s.cache.Del(ctx, userIDKeyPrefix+params.ID)
		if params.Email != nil && *params.Email != "" {
			_ = s.cache.Del(ctx, "user:email:"+*params.Email)
		}
	}

	// Publish update event
	changes := make(map[string]any)
	if params.Email != nil {
		changes["email"] = *params.Email
	}
	if params.Name != nil {
		changes["name"] = *params.Name
	}
	if params.Avatar != nil {
		changes["avatar"] = *params.Avatar
	}
	if len(changes) > 0 {
		s.publishUserUpdated(ctx, updated.ID, updated.Email, changes)
	}

	return updated, nil
}

// DeleteUser deletes a user
func (s *Service) DeleteUser(ctx context.Context, id string) error {
	if id == "" {
		return fmt.Errorf("user ID is required")
	}

	// Fetch user before deletion to publish event with email
	user, getErr := s.repo.GetByID(ctx, id)

	if err := s.repo.Delete(ctx, id); err != nil {
		return err
	}

	// Invalidate cache
	if s.cache != nil {
		_ = s.cache.Del(ctx, userIDKeyPrefix+id)
	}

	// Publish deletion event
	if getErr == nil && user != nil {
		s.publishUserDeleted(ctx, user.ID, user.Email)
	}
	return nil
}

// ListUsers lists users with pagination
func (s *Service) ListUsers(ctx context.Context, params ListUsersParams) ([]*User, int, error) {
	// Set default pagination
	if params.Page < 1 {
		params.Page = 1
	}
	if params.Limit < 1 || params.Limit > 100 {
		params.Limit = 20
	}

	return s.repo.List(ctx, params)
}

// ActivateUser activates a user
func (s *Service) ActivateUser(ctx context.Context, userID string) (*User, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	user, err := s.repo.GetByID(ctx, userID)
	if err != nil {
		return nil, err
	}

	if user.Status == UserStatusActive {
		return user, nil // Already active
	}

	return s.repo.UpdateStatus(ctx, userID, UserStatusActive)
}

// DeactivateUser deactivates a user
func (s *Service) DeactivateUser(ctx context.Context, userID string) (*User, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	return s.repo.UpdateStatus(ctx, userID, UserStatusInactive)
}

// BlockUser blocks a user
func (s *Service) BlockUser(ctx context.Context, userID, reason string) (*User, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	if reason == "" {
		return nil, fmt.Errorf("reason is required")
	}

	// TODO: Log the block reason in activities

	return s.repo.UpdateStatus(ctx, userID, UserStatusBlocked)
}

// UnblockUser unblocks a user
func (s *Service) UnblockUser(ctx context.Context, userID string) (*User, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	user, err := s.repo.GetByID(ctx, userID)
	if err != nil {
		return nil, err
	}

	if user.Status != UserStatusBlocked {
		return nil, fmt.Errorf("user is not blocked")
	}

	return s.repo.UpdateStatus(ctx, userID, UserStatusActive)
}

// SuspendUser suspends a user
func (s *Service) SuspendUser(ctx context.Context, userID, reason string) (*User, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	if reason == "" {
		return nil, fmt.Errorf("reason is required")
	}

	// TODO: Log the suspension reason in activities
	// TODO: Handle suspension expiry

	return s.repo.UpdateStatus(ctx, userID, UserStatusSuspended)
}

// UnsuspendUser unsuspends a user
func (s *Service) UnsuspendUser(ctx context.Context, userID string) (*User, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	user, err := s.repo.GetByID(ctx, userID)
	if err != nil {
		return nil, err
	}

	if user.Status != UserStatusSuspended {
		return nil, fmt.Errorf("user is not suspended")
	}

	return s.repo.UpdateStatus(ctx, userID, UserStatusActive)
}

// GetUserProfile retrieves a user profile
func (s *Service) GetUserProfile(ctx context.Context, userID string) (*UserProfile, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	// Verify user exists
	_, err := s.repo.GetByID(ctx, userID)
	if err != nil {
		return nil, err
	}

	return s.repo.GetProfile(ctx, userID)
}

// UpdateUserProfile updates a user profile
func (s *Service) UpdateUserProfile(ctx context.Context, params UpdateProfileParams) (*UserProfile, error) {
	if params.UserID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	// Verify user exists
	_, err := s.repo.GetByID(ctx, params.UserID)
	if err != nil {
		return nil, err
	}

	return s.repo.UpdateProfile(ctx, params)
}

// UpdateLastLogin updates a user's last login timestamp
func (s *Service) UpdateLastLogin(ctx context.Context, userID string) error {
	if userID == "" {
		return fmt.Errorf("user ID is required")
	}

	return s.repo.UpdateLastLogin(ctx, userID)
}

// VerifyPassword verifies a password against a user's hash
func (s *Service) VerifyPassword(ctx context.Context, email, password string) (*User, error) {
	if email == "" || password == "" {
		return nil, fmt.Errorf("email and password are required")
	}

	user, err := s.repo.GetByEmail(ctx, email)
	if err != nil {
		return nil, err
	}

	if user.PasswordHash == "" {
		return nil, fmt.Errorf("user has no password set")
	}

	err = bcrypt.CompareHashAndPassword([]byte(user.PasswordHash), []byte(password))
	if err != nil {
		return nil, fmt.Errorf("invalid password")
	}

	return user, nil
}

// ============================================
// SETTINGS SERVICE METHODS
// ============================================

// GetSettings retrieves a category's settings for a user, returning defaults if no row exists.
func (s *Service) GetSettings(ctx context.Context, userID, category string, defaults map[string]interface{}) (map[string]interface{}, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	row, err := s.repo.GetSettings(ctx, userID, category)
	if err != nil {
		return nil, err
	}
	if row == nil {
		return defaults, nil
	}
	// Merge: start from defaults so any new keys added in future defaults are present
	merged := make(map[string]interface{}, len(defaults))
	for k, v := range defaults {
		merged[k] = v
	}
	for k, v := range row.Settings {
		merged[k] = v
	}
	return merged, nil
}

// UpsertSettings saves a partial or full settings map for a user + category.
func (s *Service) UpsertSettings(ctx context.Context, userID, category string, settings map[string]interface{}) (map[string]interface{}, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	row, err := s.repo.UpsertSettings(ctx, UpsertSettingsParams{
		UserID:   userID,
		Category: category,
		Settings: settings,
	})
	if err != nil {
		return nil, err
	}
	return row.Settings, nil
}

// ============================================
// PROVIDER ACCOUNT SERVICE METHODS
// ============================================

// LinkProviderAccount upserts a social-login provider account for a user.
// Called from the NATS event handler on every successful social sign-in.
func (s *Service) LinkProviderAccount(ctx context.Context, params UpsertProviderAccountParams) (*ProviderAccount, error) {
	if params.UserID == "" || params.Provider == "" || params.ProviderUserID == "" {
		return nil, fmt.Errorf("userID, provider, and providerUserID are required")
	}
	// Verify local user exists
	if _, err := s.repo.GetByID(ctx, params.UserID); err != nil {
		return nil, fmt.Errorf("cannot link provider: %w", err)
	}

	pa, err := s.repo.UpsertProviderAccount(ctx, params)
	if err != nil {
		return nil, err
	}

	// Mirror language + timezone into user_profiles when available
	if params.Provider == "microsoft" || params.Provider == "google" {
		profileParams := UpdateProfileParams{UserID: params.UserID}
		if tz, ok := params.Metadata["timeZone"].(string); ok && tz != "" {
			profileParams.Timezone = &tz
		}
		if lang, ok := params.Metadata["preferredLanguage"].(string); ok && lang != "" {
			profileParams.Language = &lang
		}
		// Best-effort: ignore profile errors so provider link always succeeds
		_, _ = s.repo.UpdateProfile(ctx, profileParams)
	}

	// Fetch user to get email for event
	if user, uErr := s.repo.GetByID(ctx, params.UserID); uErr == nil && user != nil {
		// Publish provider link event (Ingestion Plane subscribes to setup M365)
		tenantID := ""
		if tid, ok := params.Metadata["tenant_id"].(string); ok {
			tenantID = tid
		}
		s.publishProviderLinked(ctx, user.ID, user.Email, params.Provider, tenantID)
	}

	return pa, nil
}

// GetProviderAccounts returns all linked provider accounts for a user.
func (s *Service) GetProviderAccounts(ctx context.Context, userID string) ([]*ProviderAccount, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	return s.repo.GetProviderAccounts(ctx, userID)
}

// GetUserByProviderID resolves a local user from an OAuth provider identity.
func (s *Service) GetUserByProviderID(ctx context.Context, provider, providerUserID string) (*User, error) {
	return s.repo.GetUserByProviderID(ctx, provider, providerUserID)
}

// MarkOnboardingComplete marks a user's onboarding as complete
func (s *Service) MarkOnboardingComplete(ctx context.Context, email string) error {
	if email == "" {
		return fmt.Errorf("email is required")
	}
	return s.repo.MarkOnboardingComplete(ctx, email)
}

// MarkOnboardingCompleteByID marks a user's onboarding as complete by ID
func (s *Service) MarkOnboardingCompleteByID(ctx context.Context, userID string) error {
	if userID == "" {
		return fmt.Errorf("user ID is required")
	}
	return s.repo.MarkOnboardingCompleteByID(ctx, userID)
}

// ============================================
// ONBOARDING STATE (G3 + G16)
// ============================================

// OnboardingStateView is the server-side mirror of the wizard state.
// `Step` drives the route-resume path (G16); `State` carries the partial
// form data the wizard collects across pages so multi-device refresh can
// hydrate from the server instead of localStorage (G3).
//
// (Named `View` to avoid colliding with the existing `Service` package's
// historical `OnboardingStatus` enum strings.)
type OnboardingStateView struct {
	Step  string                 `json:"step"`
	State map[string]interface{} `json:"state,omitempty"`
}

// GetOnboardingState returns the persisted state for userID. An empty
// `Step` means the user has no in-flight wizard.
func (s *Service) GetOnboardingState(ctx context.Context, userID string) (*OnboardingStateView, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	step, raw, err := s.repo.GetOnboardingState(ctx, userID)
	if err != nil {
		return nil, err
	}
	out := &OnboardingStateView{Step: step}
	if len(raw) > 0 {
		if err := json.Unmarshal(raw, &out.State); err != nil {
			return nil, fmt.Errorf("decode onboarding state: %w", err)
		}
	}
	return out, nil
}

// UpsertOnboardingState writes the wizard step + opaque state blob. An
// empty `Step` clears the step column; a nil `State` clears the JSONB
// column. Server-side enum validation is intentionally minimal: the
// client owns the step taxonomy
// (`profile|organization|website|connect|team|complete`), the server
// stores whatever it's told. Future tightening can land as a CHECK
// constraint on the column.
func (s *Service) UpsertOnboardingState(ctx context.Context, userID string, in OnboardingStateView) error {
	if userID == "" {
		return fmt.Errorf("user ID is required")
	}
	var raw []byte
	if in.State != nil {
		b, err := json.Marshal(in.State)
		if err != nil {
			return fmt.Errorf("encode onboarding state: %w", err)
		}
		raw = b
	}
	return s.repo.UpsertOnboardingState(ctx, userID, in.Step, raw)
}

// ============================================
// API KEY SERVICE METHODS
// ============================================

// CreateAPIKey generates a new API key for a user, stores the bcrypt hash,
// and returns the one-time plaintext key in APIKeyCreateResult.RawKey.
func (s *Service) CreateAPIKey(ctx context.Context, params CreateAPIKeyParams) (*APIKeyCreateResult, error) {
	if params.UserID == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	if params.Name == "" {
		return nil, fmt.Errorf("key name is required")
	}

	// Verify the user exists
	if _, err := s.repo.GetByID(ctx, params.UserID); err != nil {
		return nil, fmt.Errorf("user not found: %w", err)
	}

	// Generate a 32-byte random key, format: sk_{first8hex}_{remaining56hex}
	rawBytes := make([]byte, 32)
	if _, err := rand.Read(rawBytes); err != nil {
		return nil, fmt.Errorf("failed to generate key entropy: %w", err)
	}
	rawHex := hex.EncodeToString(rawBytes) // 64 hex chars
	keyPrefix := "sk_" + rawHex[:8]
	rawKey := keyPrefix + "_" + rawHex[8:]

	// Hash with bcrypt (key is long enough that cost 10 is fine)
	hash, err := bcrypt.GenerateFromPassword([]byte(rawKey), bcrypt.DefaultCost)
	if err != nil {
		return nil, fmt.Errorf("failed to hash key: %w", err)
	}

	k, err := s.repo.CreateAPIKey(ctx,
		params.UserID, params.Name, params.Description,
		string(hash), keyPrefix, params.Scopes, params.ExpiresAt,
	)
	if err != nil {
		return nil, err
	}

	return &APIKeyCreateResult{Key: k, RawKey: rawKey}, nil
}

// ListAPIKeys returns all API keys for a user (active and revoked).
func (s *Service) ListAPIKeys(ctx context.Context, userID string) ([]*APIKey, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	return s.repo.ListAPIKeys(ctx, userID)
}

// RevokeAPIKey marks an API key as revoked. Only the owning user can revoke.
func (s *Service) RevokeAPIKey(ctx context.Context, userID, keyID string) error {
	if userID == "" || keyID == "" {
		return fmt.Errorf("user ID and key ID are required")
	}
	return s.repo.RevokeAPIKey(ctx, userID, keyID)
}

// EnsureMembership upserts membership and applies deterministic default role rules.
func (s *Service) EnsureMembership(ctx context.Context, params EnsureMembershipParams) (*UserOrgMembership, error) {
	if params.UserID == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	if params.OrgID == "" {
		return nil, fmt.Errorf("org ID is required")
	}

	role := strings.ToLower(strings.TrimSpace(params.Role))
	if role == "" {
		count, err := s.repo.CountActiveOrgMemberships(ctx, params.OrgID)
		if err != nil {
			return nil, err
		}
		if count == 0 {
			role = "owner"
		} else {
			role = "member"
		}
	}

	status := strings.ToLower(strings.TrimSpace(params.Status))
	if status == "" {
		status = "active"
	}

	return s.repo.EnsureUserOrgMembership(ctx, EnsureMembershipParams{
		UserID: params.UserID,
		OrgID:  params.OrgID,
		Role:   role,
		Status: status,
	})
}

// ============================================
// EVENT PUBLISHING METHODS
// ============================================

// publishUserRegistered publishes aqencia.controlplane.user.registered.
// Ingestion Plane subscribes to setup M365 and other provider integrations.
func (s *Service) publishUserRegistered(ctx context.Context, userID, email, name, provider string) {
	if s.sharedPublisher == nil {
		return
	}
	s.sharedPublisher.PublishUserRegistered(ctx, userID, email, name, provider)
}

// publishUserUpdated publishes aqencia.controlplane.user.updated.
func (s *Service) publishUserUpdated(ctx context.Context, userID, email string, changes map[string]any) {
	if s.sharedPublisher == nil {
		return
	}
	s.sharedPublisher.PublishUserUpdated(ctx, userID, email, changes)
}

// publishUserDeleted publishes aqencia.controlplane.user.deleted.
func (s *Service) publishUserDeleted(ctx context.Context, userID, email string) {
	if s.sharedPublisher == nil {
		return
	}
	s.sharedPublisher.PublishUserDeleted(ctx, userID, email)
}

// publishProviderLinked publishes aqencia.controlplane.user.provider_linked.
// Ingestion Plane subscribes to setup M365, Google, and other cloud integrations.
func (s *Service) publishProviderLinked(ctx context.Context, userID, email, provider, tenantID string) {
	if s.sharedPublisher == nil {
		return
	}
	s.sharedPublisher.PublishProviderLinked(ctx, userID, email, provider, tenantID)
}

// GetSessionContext returns the post-login routing context for frontend.
//
// The optional (email, name, avatar) hints are forwarded from velion's edge
// gate via `X-User-{Email,Name,Avatar}` headers. They unlock auto-provisioning
// on first sign-in: when the user exists in auth-service but not yet in
// user-service, plain `GetByID` returns "user not found" and the entire
// cascade — dashboard ↔ OnboardingGuard ↔ session-core proxy — collapses
// into a 500/502/onboarding-restart loop. Mirrors the auto-provision contract
// already used by `getCurrentUserProfile`.
func (s *Service) GetSessionContext(ctx context.Context, userID, email, name, avatar string) (*SessionContext, error) {
	if userID == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	user, err := s.GetOrCreateUser(ctx, userID, email, name, avatar)
	if err != nil {
		return nil, err
	}

	ctxOut := &SessionContext{
		UserID:           user.ID,
		OnboardingStatus: "PROFILE_READY",
	}

	if user.OnboardingComplete {
		ctxOut.OnboardingStatus = "COMPLETED"
	}

	membership, err := s.repo.GetPrimaryUserOrgMembership(ctx, userID)
	if err != nil {
		return nil, err
	}
	if membership == nil {
		ctxOut.OnboardingStatus = "CREATED"
		return ctxOut, nil
	}

	ctxOut.OrgID = membership.OrgID
	ctxOut.Role = membership.Role
	return ctxOut, nil
}
