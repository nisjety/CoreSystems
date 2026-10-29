package handlers

import (
	"context"
	"log"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/clients"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/nats"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// EventHandler handles incoming NATS events from auth-service
type EventHandler struct {
	userRepo        *users.Repository
	publisher       *nats.Publisher
	sharedPublisher *nats.SharedPublisher // cross-plane events on verevon-nats

	// G41 (Slice D) — Microsoft Graph enrichment dependencies.
	// Both nil-safe: when either is absent (e.g. local dev without an
	// internal API key), `HandleUserProviderLinked` skips the Graph fetch
	// and falls back to whatever ProfileHints auth-core supplied.
	authCoreOAuth *clients.AuthCoreOAuthClient
	graphClient   *clients.MicrosoftGraphClient
}

// NewEventHandler creates a new event handler
func NewEventHandler(
	userRepo *users.Repository,
	publisher *nats.Publisher,
	sharedPublisher *nats.SharedPublisher,
	authCoreOAuth *clients.AuthCoreOAuthClient,
	graphClient *clients.MicrosoftGraphClient,
) *EventHandler {
	return &EventHandler{
		userRepo:        userRepo,
		publisher:       publisher,
		sharedPublisher: sharedPublisher,
		authCoreOAuth:   authCoreOAuth,
		graphClient:     graphClient,
	}
}

// isSocialProvider returns true for OAuth/social providers (not email/password)
func isSocialProvider(provider string) bool {
	switch provider {
	case "", "email", "password", "credentials", "magic-link":
		return false
	default:
		return true // microsoft, google, github, apple, etc.
	}
}

// HandleUserRegistered handles user.registered events from auth-service
func (h *EventHandler) HandleUserRegistered(ctx context.Context, event *nats.UserRegisteredEvent) error {
	log.Printf("🔔 Handling user registered: %s (provider: %s)", event.Email, event.Provider)

	existingUser, err := h.userRepo.GetByEmail(ctx, event.Email)
	if (err != nil || existingUser == nil) && event.UserID != "" {
		if byIDUser, byIDErr := h.userRepo.GetByID(ctx, event.UserID); byIDErr == nil && byIDUser != nil {
			existingUser = byIDUser
			err = nil
		}
	}

	if err == nil && existingUser != nil {
		// User already exists in user-core — still upsert provider account if social
		if isSocialProvider(event.Provider) {
			h.upsertProviderFromRegistered(ctx, existingUser.ID, event)
		}
		log.Printf("ℹ️  User already exists: %s", event.Email)
		return nil
	}

	// User doesn't exist yet — create them in the local DB using the auth userId as PK
	avatar := ""
	if event.ProfileHints != nil {
		avatar = strings.TrimSpace(event.ProfileHints.Avatar)
	}

	newUser, createErr := h.userRepo.CreateWithID(ctx, event.UserID, users.CreateUserParams{
		Email:  event.Email,
		Name:   event.Name,
		Avatar: avatar,
	})
	if createErr != nil {
		log.Printf("❌ Failed to create user in local DB on NATS event: %v", createErr)
		return createErr
	}

	log.Printf("✅ Created user in local DB via NATS: %s (id=%s)", event.Email, newUser.ID)

	// Publish to shared cross-plane NATS (verevon-nats) so other planes can react
	if h.sharedPublisher != nil {
		h.sharedPublisher.PublishUserRegistered(ctx, newUser.ID, event.Email, event.Name, event.Provider)
	}

	// Upsert provider account if using a social/OAuth provider
	if isSocialProvider(event.Provider) {
		h.upsertProviderFromRegistered(ctx, newUser.ID, event)
	}

	return nil
}

// upsertProviderFromRegistered extracts provider info from a registration event and persists it
func (h *EventHandler) upsertProviderFromRegistered(ctx context.Context, userCoreID string, event *nats.UserRegisteredEvent) {
	providerUserID := strings.TrimSpace(event.UserID)
	if providerUserID == "" && event.Metadata != nil {
		if accountID, ok := event.Metadata["accountId"].(string); ok && strings.TrimSpace(accountID) != "" {
			providerUserID = strings.TrimSpace(accountID)
		}
	}
	if providerUserID == "" {
		providerUserID = strings.TrimSpace(event.TokenRef)
	}
	if providerUserID == "" {
		providerUserID = userCoreID
	}

	displayName := event.Name
	if event.ProfileHints != nil && strings.TrimSpace(event.ProfileHints.DisplayName) != "" {
		displayName = strings.TrimSpace(event.ProfileHints.DisplayName)
	}
	if displayName == "" && event.Metadata != nil {
		if dn, ok := event.Metadata["displayName"].(string); ok && dn != "" {
			displayName = dn
		}
	}

	emailFromProvider := strings.TrimSpace(event.EmailFromProvider)
	if emailFromProvider == "" {
		emailFromProvider = strings.TrimSpace(event.Email)
	}

	metadata := map[string]interface{}{}
	for key, value := range event.Metadata {
		metadata[key] = value
	}
	if event.ProfileHints != nil {
		if avatar := strings.TrimSpace(event.ProfileHints.Avatar); avatar != "" {
			metadata["avatar"] = avatar
		}
		if locale := strings.TrimSpace(event.ProfileHints.Locale); locale != "" {
			metadata["locale"] = locale
		}
		if timezone := strings.TrimSpace(event.ProfileHints.Timezone); timezone != "" {
			metadata["timeZone"] = timezone
		}
	}

	tokenRef := strings.TrimSpace(event.TokenRef)
	if tokenRef == "" {
		tokenRef = providerUserID
	}

	params := users.UpsertProviderAccountParams{
		UserID:            userCoreID,
		Provider:          event.Provider,
		ProviderUserID:    providerUserID,
		TenantID:          event.TenantID,
		MicrosoftTenantID: event.MicrosoftTenantID,
		Email:             event.Email,
		EmailFromProvider: emailFromProvider,
		DisplayName:       displayName,
		ScopesGranted:     event.ScopesGranted,
		TokenRef:          tokenRef,
		Metadata:          metadata,
	}

	if _, err := h.userRepo.UpsertProviderAccount(ctx, params); err != nil {
		log.Printf("⚠️  Failed to upsert provider account on registration: %v", err)
		return
	}

	user, err := h.userRepo.GetByID(ctx, userCoreID)
	if err != nil || user == nil {
		log.Printf("⚠️  Failed to resolve user after provider registration upsert: %v", err)
		return
	}
	h.publishProviderReadyForIntegration(ctx, user, params)
}
func (h *EventHandler) HandleUserLogin(ctx context.Context, event *nats.UserLoginEvent) error {
	log.Printf("🔔 Handling user login: %s (provider: %s)", event.Email, event.Provider)

	user, err := h.userRepo.GetByEmail(ctx, event.Email)
	if err != nil {
		log.Printf("❌ User not found for login event: %s", event.Email)
		return err
	}

	// Update last login timestamp
	if err := h.userRepo.UpdateLastLogin(ctx, user.ID); err != nil {
		log.Printf("❌ Failed to update last login: %v", err)
		return err
	}

	// Upsert provider account on every social login so metadata stays fresh
	if isSocialProvider(event.Provider) {
		if _, err := h.userRepo.UpsertProviderAccount(ctx, users.UpsertProviderAccountParams{
			UserID:         user.ID,
			Provider:       event.Provider,
			ProviderUserID: event.UserID, // auth-service stable user ID
			Email:          event.Email,
			Metadata: map[string]interface{}{
				"deviceInfo": event.DeviceInfo,
				"ipAddress":  event.IPAddress,
				"userAgent":  event.UserAgent,
			},
		}); err != nil {
			log.Printf("⚠️  Failed to upsert provider account on login: %v", err)
			// Non-fatal — don't block the login flow
		}
	}

	// Log activity
	details := map[string]interface{}{
		"provider":  event.Provider,
		"sessionId": event.SessionID,
	}
	if event.DeviceInfo != "" {
		details["deviceInfo"] = event.DeviceInfo
	}
	_ = h.publisher.PublishActivityLogged(
		ctx, user.ID, "login", "session",
		event.IPAddress, event.UserAgent, details,
	)

	log.Printf("✅ User login processed: %s", event.Email)
	return nil
}

// HandleUserLogout handles user.logout events from auth-service
func (h *EventHandler) HandleUserLogout(ctx context.Context, event *nats.UserLogoutEvent) error {
	log.Printf("🔔 Handling user logout: %s", event.Email)

	// Get user
	user, err := h.userRepo.GetByEmail(ctx, event.Email)
	if err != nil {
		log.Printf("⚠️  User not found for logout event: %s", event.Email)
		return nil // Don't fail - user might have been deleted
	}

	// Log activity
	details := map[string]interface{}{
		"sessionId": event.SessionID,
		"reason":    event.Reason,
	}

	_ = h.publisher.PublishActivityLogged(
		ctx,
		user.ID,
		"logout",
		"session",
		"",
		"",
		details,
	)

	log.Printf("✅ User logout processed: %s", event.Email)
	return nil
}

// HandleUserProfileUpdated handles user.profile_updated events from auth-service
func (h *EventHandler) HandleUserProfileUpdated(ctx context.Context, event *nats.UserProfileUpdatedEvent) error {
	log.Printf("🔔 Handling user profile updated: %s", event.Email)

	// Get user
	user, err := h.userRepo.GetByEmail(ctx, event.Email)
	if err != nil {
		log.Printf("⚠️  User not found for profile update event: %s (skipping)", event.Email)
		return nil
	}

	// Update user fields if present in changes
	updated := false
	var name, avatar string
	if n, ok := event.Changes["name"].(string); ok {
		name = n
		user.Name = name
		updated = true
	}
	if av, ok := event.Changes["image"].(string); ok {
		avatar = av
		user.Avatar = avatar
		updated = true
	}
	if emailVerified, ok := event.Changes["emailVerified"].(bool); ok {
		user.EmailVerified = emailVerified
		updated = true
	}

	if updated {
		updateParams := users.UpdateUserParams{
			ID:     user.ID,
			Name:   &user.Name,
			Avatar: &user.Avatar,
		}

		if _, err := h.userRepo.Update(ctx, updateParams); err != nil {
			log.Printf("❌ Failed to update user profile: %v", err)
			return err
		}

		// Publish profile updated event
		_ = h.publisher.PublishProfileUpdated(ctx, user.ID, event.Changes)
	}

	log.Printf("✅ User profile updated: %s", event.Email)
	return nil
}

// HandleSessionCreated handles session.created events from auth-service
func (h *EventHandler) HandleSessionCreated(ctx context.Context, event *nats.SessionCreatedEvent) error {
	log.Printf("🔔 Handling session created: %s", event.SessionID)

	// Get user
	user, err := h.userRepo.GetByEmail(ctx, event.Email)
	if err != nil {
		log.Printf("⚠️  User not found for session created event: %s", event.Email)
		return nil
	}

	// Publish session created event
	_ = h.publisher.PublishSessionCreated(
		ctx,
		event.SessionID,
		user.ID,
		event.DeviceInfo,
		event.IPAddress,
		event.UserAgent,
		event.ExpiresAt,
	)

	log.Printf("✅ Session created processed: %s", event.SessionID)
	return nil
}

// HandleSessionEnded handles session.ended events from auth-service
func (h *EventHandler) HandleSessionEnded(ctx context.Context, event *nats.SessionEndedEvent) error {
	log.Printf("🔔 Handling session ended: %s", event.SessionID)

	// Get user
	user, err := h.userRepo.GetByEmail(ctx, event.Email)
	if err != nil {
		log.Printf("⚠️  User not found for session ended event: %s", event.Email)
		return nil
	}

	// Log activity
	details := map[string]interface{}{
		"sessionId": event.SessionID,
		"reason":    event.Reason,
	}

	_ = h.publisher.PublishActivityLogged(
		ctx,
		user.ID,
		"session_ended",
		"session",
		"",
		"",
		details,
	)

	log.Printf("✅ Session ended processed: %s", event.SessionID)
	return nil
}

// HandleUserProviderLinked handles provider_linked events from auth-service.
// Fired when an existing user links a new OAuth provider to their account.
func (h *EventHandler) HandleUserProviderLinked(ctx context.Context, event *nats.UserProviderLinkedEvent) error {
	log.Printf("🔔 Handling provider linked: %s → %s", event.Email, event.Provider)

	// G48 (verevon-gap.md §8.33): GetByID fallback when GetByEmail fails.
	// Mirrors the lookup pattern in HandleUserRegistered. Necessary because
	// the legacy auto-provision flow can leave user_service.users rows under
	// a placeholder email (e.g. `g3-smoke@example.com`) while auth-service
	// emits the real provider email on link. Without this fallback the
	// Graph enrichment path silently aborts and §8.33 G46 cannot land.
	user, err := h.userRepo.GetByEmail(ctx, event.Email)
	if (err != nil || user == nil) && event.UserID != "" {
		if byIDUser, byIDErr := h.userRepo.GetByID(ctx, event.UserID); byIDErr == nil && byIDUser != nil {
			user = byIDUser
			err = nil
		}
	}
	if err != nil || user == nil {
		log.Printf("⚠️  User not found for provider_linked event: %s (user_id=%s)", event.Email, event.UserID)
		return nil
	}

	providerUserID := event.ProviderAccountID
	if providerUserID == "" {
		providerUserID = event.UserID // fall back to auth-service user ID
	}

	metadata := map[string]interface{}{"linkedAt": event.Timestamp}
	displayName := ""
	if event.ProfileHints != nil {
		displayName = strings.TrimSpace(event.ProfileHints.DisplayName)
		if avatar := strings.TrimSpace(event.ProfileHints.Avatar); avatar != "" {
			metadata["avatar"] = avatar
		}
		if locale := strings.TrimSpace(event.ProfileHints.Locale); locale != "" {
			metadata["locale"] = locale
		}
		if timezone := strings.TrimSpace(event.ProfileHints.Timezone); timezone != "" {
			metadata["timeZone"] = timezone
		}
	}

	if displayName == "" {
		displayName = strings.TrimSpace(event.Email)
	}

	params := users.UpsertProviderAccountParams{
		UserID:            user.ID,
		Provider:          event.Provider,
		ProviderUserID:    providerUserID,
		TenantID:          event.TenantID,
		MicrosoftTenantID: event.MicrosoftTenantID,
		Email:             event.Email,
		EmailFromProvider: event.EmailFromProvider,
		DisplayName:       displayName,
		ScopesGranted:     event.ScopesGranted,
		TokenRef:          event.TokenRef,
		Metadata:          metadata,
	}

	if _, err := h.userRepo.UpsertProviderAccount(ctx, params); err != nil {
		log.Printf("❌ Failed to upsert provider account on link: %v", err)
		return err
	}

	// G41 (Slice D): for Microsoft accounts, fetch Graph profile + photo and
	// soft-update the local user/profile rows. Best-effort — failures are
	// logged and swallowed so the provider-link row still persists and the
	// downstream `provider_ready_for_integration` event still fires.
	h.enrichFromMicrosoftGraph(ctx, user, event)

	h.publishProviderReadyForIntegration(ctx, user, params)

	log.Printf("✅ Provider linked stored: %s → %s", event.Email, event.Provider)
	return nil
}

// enrichFromMicrosoftGraph performs the Slice D "Graph enrichment" step.
// It is intentionally best-effort: every early-return path logs why it
// skipped, and any HTTP failure is logged + swallowed so the calling event
// handler still returns nil.
//
// Reads from:
//   - auth-core `POST /internal/oauth/token` (via clients.AuthCoreOAuthClient)
//   - Microsoft Graph `GET /v1.0/me` + `GET /v1.0/me/photo/$value`
//
// Writes to:
//   - `users` (display name, avatar data-URL)
//   - `user_profiles` (location, timezone, language, jobTitle, mobilePhone,
//     businessPhones — all via the metadata bag)
func (h *EventHandler) enrichFromMicrosoftGraph(ctx context.Context, user *users.User, event *nats.UserProviderLinkedEvent) {
	if user == nil || event == nil {
		return
	}
	if !strings.EqualFold(event.Provider, "microsoft") {
		return
	}
	if h.authCoreOAuth == nil || h.graphClient == nil {
		log.Printf("ℹ️  Graph enrichment skipped (clients not configured): %s", event.Email)
		return
	}
	tokenRef := strings.TrimSpace(event.TokenRef)
	if tokenRef == "" {
		log.Printf("ℹ️  Graph enrichment skipped (no tokenRef): %s", event.Email)
		return
	}

	// Step 1: exchange tokenRef → access_token.
	tokenResult, err := h.authCoreOAuth.GetTokenByRef(ctx, tokenRef)
	if err != nil {
		log.Printf("⚠️  Graph enrichment: auth-core token fetch failed (%s): %v", event.Email, err)
		return
	}

	// Step 2: fetch the Graph /me profile.
	// On 401 (expired token), attempt one refresh via auth-core's
	// `/internal/oauth/refresh` (G24) and retry. Microsoft Entra access
	// tokens expire after ~60min — without this, every event for a
	// returning user whose last sign-in was >1h ago would skip enrichment.
	profile, err := h.graphClient.GetMe(ctx, tokenResult.AccessToken)
	if err != nil && isGraphAuthError(err) {
		log.Printf("ℹ️  Graph enrichment: access token rejected (%s) — attempting refresh", event.Email)
		refreshed, refreshErr := h.authCoreOAuth.RefreshTokenByRef(ctx, tokenRef)
		if refreshErr != nil {
			log.Printf("⚠️  Graph enrichment: refresh failed (%s): %v", event.Email, refreshErr)
			return
		}
		tokenResult = refreshed
		profile, err = h.graphClient.GetMe(ctx, tokenResult.AccessToken)
	}
	if err != nil {
		log.Printf("⚠️  Graph enrichment: /me fetch failed (%s): %v", event.Email, err)
		return
	}

	// Step 3: fetch the photo (404/missing is fine).
	photo, photoErr := h.graphClient.GetPhotoValue(ctx, tokenResult.AccessToken)
	if photoErr != nil {
		// Log but don't bail — non-fatal; many users have no photo set.
		log.Printf("ℹ️  Graph enrichment: /me/photo skipped (%s): %v", event.Email, photoErr)
	}

	// Step 4: persist the enrichment.
	displayName := pickName(profile)
	updateParams := users.UpdateUserParams{ID: user.ID}
	if displayName != "" && displayName != user.Name {
		nameCopy := displayName
		updateParams.Name = &nameCopy
	}
	if photo != nil {
		if dataURL := photo.DataURL(); dataURL != "" && dataURL != user.Avatar {
			avatarCopy := dataURL
			updateParams.Avatar = &avatarCopy
		}
	}
	// G46 (verevon-gap.md §8.32): when Graph returns a more authoritative
	// email than what's currently stored (auto-provisioned placeholders
	// like `g3-smoke@example.com` from a Wave-9-era fixture), refresh the
	// `users.email` column so downstream lookups by email find the right
	// row. Microsoft Graph exposes the canonical work email via the
	// `mail` field; `userPrincipalName` is the UPN fallback when `mail`
	// is unset on the tenant.
	graphEmail := strings.TrimSpace(profile.Mail)
	if graphEmail == "" {
		graphEmail = strings.TrimSpace(profile.UserPrincipalName)
	}
	if graphEmail != "" && !strings.EqualFold(graphEmail, user.Email) {
		emailCopy := graphEmail
		updateParams.Email = &emailCopy
	}
	if updateParams.Name != nil || updateParams.Avatar != nil || updateParams.Email != nil {
		if _, err := h.userRepo.Update(ctx, updateParams); err != nil {
			log.Printf("⚠️  Graph enrichment: users update failed (%s): %v", event.Email, err)
			// fall through — try the profile update anyway
		}
	}

	// Step 5: update the extended profile.
	profileParams := users.UpdateProfileParams{UserID: user.ID, Metadata: graphMetadata(profile)}
	if loc := strings.TrimSpace(profile.OfficeLocation); loc != "" {
		profileParams.Location = &loc
	}
	if lang := strings.TrimSpace(profile.PreferredLanguage); lang != "" {
		profileParams.Language = &lang
	}
	if phone := strings.TrimSpace(profile.MobilePhone); phone != "" {
		profileParams.Phone = &phone
	}
	if _, err := h.userRepo.UpdateProfile(ctx, profileParams); err != nil {
		log.Printf("⚠️  Graph enrichment: profile update failed (%s): %v", event.Email, err)
		return
	}

	log.Printf("✅ Graph enrichment applied: %s (displayName=%q jobTitle=%q hasPhoto=%v)",
		event.Email, displayName, profile.JobTitle, photo != nil)
}

// pickName returns the best display name available on the Graph profile,
// preferring `displayName`, then `givenName + surname`, then UPN, then mail.
func pickName(profile *clients.GraphProfile) string {
	if profile == nil {
		return ""
	}
	if name := strings.TrimSpace(profile.DisplayName); name != "" {
		return name
	}
	first := strings.TrimSpace(profile.GivenName)
	last := strings.TrimSpace(profile.Surname)
	if first != "" || last != "" {
		return strings.TrimSpace(first + " " + last)
	}
	if upn := strings.TrimSpace(profile.UserPrincipalName); upn != "" {
		return upn
	}
	return strings.TrimSpace(profile.Mail)
}

// isGraphAuthError returns true when a Graph error string carries the
// distinctive "rejected access token (401)" prefix emitted by
// `clients.MicrosoftGraphClient`. Cheap substring match — we don't depend
// on the wrapped error chain because the underlying error is constructed
// via `fmt.Errorf` not `errors.New + Wrap`.
func isGraphAuthError(err error) bool {
	if err == nil {
		return false
	}
	msg := err.Error()
	return strings.Contains(msg, "rejected access token (401)") ||
		strings.Contains(msg, "rejected access token (403)")
}

// graphMetadata folds Graph fields that don't fit the typed columns into the
// `user_profiles.metadata` JSONB bag so they're queryable without a schema
// change. Skipping empty strings keeps the metadata clean for UI consumers.
func graphMetadata(profile *clients.GraphProfile) map[string]interface{} {
	if profile == nil {
		return nil
	}
	m := make(map[string]interface{})
	if v := strings.TrimSpace(profile.JobTitle); v != "" {
		m["jobTitle"] = v
	}
	if v := strings.TrimSpace(profile.Department); v != "" {
		m["department"] = v
	}
	if v := strings.TrimSpace(profile.Mail); v != "" {
		m["graphMail"] = v
	}
	if v := strings.TrimSpace(profile.UserPrincipalName); v != "" {
		m["userPrincipalName"] = v
	}
	if len(profile.BusinessPhones) > 0 {
		m["businessPhones"] = profile.BusinessPhones
	}
	m["graphEnrichedAt"] = time.Now().UTC().Format(time.RFC3339)
	return m
}

func (h *EventHandler) publishProviderReadyForIntegration(
	ctx context.Context,
	user *users.User,
	params users.UpsertProviderAccountParams,
) {
	if h.sharedPublisher == nil || user == nil || !isSocialProvider(params.Provider) {
		return
	}

	onboardingStatus := "PROFILE_READY"
	if user.OnboardingComplete {
		onboardingStatus = "COMPLETED"
	}

	orgID := ""
	role := ""
	membership, err := h.userRepo.GetPrimaryUserOrgMembership(ctx, user.ID)
	if err != nil {
		log.Printf("⚠️  Failed to resolve primary org membership for integration-ready event: %v", err)
		return
	}
	if membership == nil {
		onboardingStatus = "CREATED"
	} else {
		orgID = strings.TrimSpace(membership.OrgID)
		role = strings.TrimSpace(membership.Role)
	}

	h.sharedPublisher.PublishProviderReadyForIntegration(
		ctx,
		user.ID,
		user.Email,
		strings.TrimSpace(params.Provider),
		strings.TrimSpace(params.ProviderUserID),
		strings.TrimSpace(params.TenantID),
		strings.TrimSpace(params.MicrosoftTenantID),
		params.ScopesGranted,
		strings.TrimSpace(params.TokenRef),
		orgID,
		role,
		onboardingStatus,
	)
}

func (h *EventHandler) HandleOrganizationMemberAdded(ctx context.Context, event *nats.OrganizationMembershipEvent) error {
	if event == nil {
		return nil
	}

	userID := strings.TrimSpace(event.UserID)
	orgID := strings.TrimSpace(event.OrganizationID)
	if userID == "" || orgID == "" {
		return nil
	}
	if userID == "pending" || strings.HasPrefix(userID, "invite_") {
		log.Printf("ℹ️  Skipping membership sync for pending invite user: %s", userID)
		return nil
	}

	user, err := h.userRepo.GetByID(ctx, userID)
	if err != nil || user == nil {
		log.Printf("⚠️  Cannot sync org membership, user missing locally: user=%s org=%s", userID, orgID)
		return nil
	}

	role := strings.ToLower(strings.TrimSpace(event.Role))
	if role == "" {
		role = "member"
	}

	if _, err := h.userRepo.EnsureUserOrgMembership(ctx, users.EnsureMembershipParams{
		UserID: user.ID,
		OrgID:  orgID,
		Role:   role,
		Status: "active",
	}); err != nil {
		log.Printf("❌ Failed to sync org membership add: user=%s org=%s err=%v", userID, orgID, err)
		return err
	}

	log.Printf("✅ Synced org membership add: user=%s org=%s role=%s", userID, orgID, role)
	return nil
}

func (h *EventHandler) HandleOrganizationMemberRemoved(ctx context.Context, event *nats.OrganizationMembershipEvent) error {
	if event == nil {
		return nil
	}

	userID := strings.TrimSpace(event.UserID)
	orgID := strings.TrimSpace(event.OrganizationID)
	if userID == "" || orgID == "" {
		return nil
	}

	if err := h.userRepo.RemoveUserOrgMembership(ctx, userID, orgID); err != nil {
		log.Printf("❌ Failed to sync org membership removal: user=%s org=%s err=%v", userID, orgID, err)
		return err
	}

	log.Printf("✅ Synced org membership removal: user=%s org=%s", userID, orgID)
	return nil
}
