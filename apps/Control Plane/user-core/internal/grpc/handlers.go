package grpc

import (
	"context"
	"log"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/clients"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/nats"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	pb "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/proto/user/v1"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// userServiceHandler implements the UserService gRPC service
type userServiceHandler struct {
	pb.UnimplementedUserServiceServer
	userService      *users.Service
	publisher        *nats.Publisher
	betterAuthClient *clients.BetterAuthClient
	repo             *users.Repository
}

// convertBetterAuthUser converts a Better Auth user to internal User type
func convertBetterAuthUser(baUser *clients.User) *users.User {
	var status users.UserStatus
	if baUser.Banned {
		status = users.UserStatusBlocked
	} else {
		status = users.UserStatusActive
	}

	user := &users.User{
		ID:            baUser.ID,
		Email:         baUser.Email,
		Name:          baUser.Name,
		EmailVerified: baUser.EmailVerified,
		Status:        status,
		CreatedAt:     baUser.CreatedAt,
		UpdatedAt:     baUser.UpdatedAt,
	}

	if baUser.Image != nil {
		user.Avatar = *baUser.Image
	}

	return user
}

// syncCanonicalAuthUser upserts a user-core row using the auth-service user ID
// as the primary key. Auth-core already owns Better Auth sign-up/sign-in, so
// this path must not create a second identity when it is called as a sync hook.
func (h *userServiceHandler) syncCanonicalAuthUser(ctx context.Context, id, email, name, avatar string) (*pb.CreateUserResponse, error) {
	if id == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	if h.betterAuthClient != nil {
		if baUser, err := h.betterAuthClient.GetUser(ctx, id); err == nil && baUser != nil {
			user := convertBetterAuthUser(baUser)
			if _, syncErr := h.userService.GetOrCreateUser(ctx, user.ID, user.Email, user.Name, user.Avatar); syncErr != nil {
				log.Printf("⚠️  Failed to sync auth user to local DB: %v", syncErr)
				return nil, status.Errorf(codes.Internal, "failed to sync user: %v", syncErr)
			}
			return &pb.CreateUserResponse{User: user.ToProto()}, nil
		}

		if email != "" {
			baUsers, _, err := h.betterAuthClient.ListUsers(ctx, &clients.ListUsersRequest{Search: &email, Limit: 1})
			if err == nil && len(baUsers) > 0 {
				if baUsers[0].ID != id {
					return nil, status.Errorf(codes.AlreadyExists, "email is already owned by another auth user")
				}
				user := convertBetterAuthUser(&baUsers[0])
				if _, syncErr := h.userService.GetOrCreateUser(ctx, user.ID, user.Email, user.Name, user.Avatar); syncErr != nil {
					log.Printf("⚠️  Failed to sync existing auth user to local DB: %v", syncErr)
					return nil, status.Errorf(codes.Internal, "failed to sync user: %v", syncErr)
				}
				return &pb.CreateUserResponse{User: user.ToProto()}, nil
			}
		}
	}

	user, err := h.userService.GetOrCreateUser(ctx, id, email, name, avatar)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to sync user: %v", err)
	}
	return &pb.CreateUserResponse{User: user.ToProto()}, nil
}

// CreateUser creates a new user
func (h *userServiceHandler) CreateUser(ctx context.Context, req *pb.CreateUserRequest) (*pb.CreateUserResponse, error) {
	id := strings.TrimSpace(req.GetId())
	email := strings.TrimSpace(req.GetEmail())
	name := strings.TrimSpace(req.GetName())
	avatar := strings.TrimSpace(req.GetAvatar())

	if id != "" {
		return h.syncCanonicalAuthUser(ctx, id, email, name, avatar)
	}

	if email == "" {
		return nil, status.Error(codes.InvalidArgument, "email is required")
	}

	if name == "" {
		return nil, status.Error(codes.InvalidArgument, "name is required")
	}

	// Create user via Better Auth
	createReq := &clients.CreateUserRequest{
		Email:         email,
		Name:          name,
		EmailVerified: false,
		Role:          "user",
	}

	if req.Password != "" {
		createReq.Password = &req.Password
	}

	if avatar != "" {
		createReq.Image = &avatar
	}

	baUser, err := h.betterAuthClient.CreateUser(ctx, createReq)
	if err != nil {
		// User likely already exists in Better Auth (auth-core syncs after its own BA sign-up).
		// Find them by email so we can still sync to the local DB.
		search := email
		baUsers, _, searchErr := h.betterAuthClient.ListUsers(ctx, &clients.ListUsersRequest{Search: &search, Limit: 1})
		if searchErr == nil && len(baUsers) > 0 {
			user := convertBetterAuthUser(&baUsers[0])
			if _, syncErr := h.userService.GetOrCreateUser(ctx, user.ID, email, name, avatar); syncErr != nil {
				log.Printf("⚠️  Failed to sync existing user to local DB: %v", syncErr)
			}
			return &pb.CreateUserResponse{User: user.ToProto()}, nil
		}
		return nil, status.Errorf(codes.Internal, "failed to create user: %v", err)
	}

	user := convertBetterAuthUser(baUser)

	// Sync to local PostgreSQL DB so HTTP by-email / by-ID lookups work
	if _, syncErr := h.userService.GetOrCreateUser(ctx, user.ID, user.Email, user.Name, user.Avatar); syncErr != nil {
		log.Printf("⚠️  Failed to sync new user to local DB: %v", syncErr)
	}

	// Publish user created event
	if h.publisher != nil {
		metadata := map[string]interface{}{
			"source": "grpc",
		}
		if err := h.publisher.PublishUserCreated(ctx, user.ID, user.Email, user.Name, string(user.Status), metadata); err != nil {
			log.Printf("⚠️  Failed to publish user created event: %v", err)
			// Don't fail the request
		}
	}

	return &pb.CreateUserResponse{
		User: user.ToProto(),
	}, nil
}

// GetUser retrieves a user by ID
func (h *userServiceHandler) GetUser(ctx context.Context, req *pb.GetUserRequest) (*pb.GetUserResponse, error) {
	if req.Id == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	// Get user from Better Auth
	baUser, err := h.betterAuthClient.GetUser(ctx, req.Id)
	if err != nil {
		return nil, status.Errorf(codes.NotFound, "user not found: %v", err)
	}

	user := convertBetterAuthUser(baUser)

	return &pb.GetUserResponse{
		User: user.ToProto(),
	}, nil
}

// GetUserByEmail retrieves a user by email
func (h *userServiceHandler) GetUserByEmail(ctx context.Context, req *pb.GetUserByEmailRequest) (*pb.GetUserByEmailResponse, error) {
	if req.Email == "" {
		return nil, status.Error(codes.InvalidArgument, "email is required")
	}

	// Search for user by email via Better Auth
	search := req.Email
	listReq := &clients.ListUsersRequest{
		Search: &search,
		Limit:  1,
	}

	users, _, err := h.betterAuthClient.ListUsers(ctx, listReq)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to search user: %v", err)
	}

	if len(users) == 0 {
		return nil, status.Error(codes.NotFound, "user not found")
	}

	user := convertBetterAuthUser(&users[0])

	return &pb.GetUserByEmailResponse{
		User: user.ToProto(),
	}, nil
}

// UpdateUser updates a user
func (h *userServiceHandler) UpdateUser(ctx context.Context, req *pb.UpdateUserRequest) (*pb.UpdateUserResponse, error) {
	if req.Id == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	// Build update request for Better Auth
	updateReq := &clients.UpdateUserRequest{
		UserID: req.Id,
	}

	if req.Name != nil {
		updateReq.Name = req.Name
	}
	if req.Avatar != nil {
		updateReq.Image = req.Avatar
	}
	if req.Email != nil {
		updateReq.Email = req.Email
	}

	baUser, err := h.betterAuthClient.UpdateUser(ctx, updateReq)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to update user: %v", err)
	}

	user := convertBetterAuthUser(baUser)

	// Sync to local PostgreSQL DB so HTTP by-email / by-ID lookups reflect current data
	if req.Email != nil && *req.Email != "" {
		syncName := user.Name
		if req.Name != nil {
			syncName = *req.Name
		}
		syncAvatar := user.Avatar
		if req.Avatar != nil {
			syncAvatar = *req.Avatar
		}
		if _, syncErr := h.userService.GetOrCreateUser(ctx, req.Id, *req.Email, syncName, syncAvatar); syncErr != nil {
			log.Printf("⚠️  Failed to sync updated user to local DB: %v", syncErr)
		}
	}

	return &pb.UpdateUserResponse{
		User: user.ToProto(),
	}, nil
}

// DeleteUser deletes a user
func (h *userServiceHandler) DeleteUser(ctx context.Context, req *pb.DeleteUserRequest) (*pb.DeleteUserResponse, error) {
	if req.Id == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	// Delete user via Better Auth
	err := h.betterAuthClient.DeleteUser(ctx, req.Id)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to delete user: %v", err)
	}

	return &pb.DeleteUserResponse{
		Success: true,
		Message: "User deleted successfully",
	}, nil
}

// ListUsers lists users with pagination
func (h *userServiceHandler) ListUsers(ctx context.Context, req *pb.ListUsersRequest) (*pb.ListUsersResponse, error) {
	page := int(req.GetPagination().GetPage())
	limit := int(req.GetPagination().GetLimit())

	if page < 1 {
		page = 1
	}
	if limit == 0 {
		limit = 10 // Default limit
	}

	// Build Better Auth list request
	listReq := &clients.ListUsersRequest{
		Limit:  limit,
		Offset: (page - 1) * limit,
	}

	// Filter by status (Better Auth uses banned field)
	if req.Status != nil {
		switch *req.Status {
		case pb.UserStatus_USER_STATUS_BLOCKED:
			banned := true
			listReq.Banned = &banned
		case pb.UserStatus_USER_STATUS_ACTIVE:
			banned := false
			listReq.Banned = &banned
		}
	}

	baUsers, total, err := h.betterAuthClient.ListUsers(ctx, listReq)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to list users: %v", err)
	}

	pbUsers := make([]*pb.User, len(baUsers))
	for i, baUser := range baUsers {
		user := convertBetterAuthUser(&baUser)
		pbUsers[i] = user.ToProto()
	}

	return &pb.ListUsersResponse{
		Users: pbUsers,
		Total: int32(total),
		Page:  int32(page),
		Limit: int32(limit),
	}, nil
}

// ActivateUser activates a user
func (h *userServiceHandler) ActivateUser(ctx context.Context, req *pb.ActivateUserRequest) (*pb.ActivateUserResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	// Unban user in Better Auth (activates them)
	unbanReq := &clients.UnbanUserRequest{
		UserID: req.UserId,
	}

	if err := h.betterAuthClient.UnbanUser(ctx, unbanReq); err != nil {
		return nil, status.Errorf(codes.Internal, "failed to activate user: %v", err)
	}

	// Get updated user
	baUser, err := h.betterAuthClient.GetUser(ctx, req.UserId)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to get user: %v", err)
	}

	user := convertBetterAuthUser(baUser)

	return &pb.ActivateUserResponse{
		User: user.ToProto(),
	}, nil
}

// DeactivateUser deactivates a user
func (h *userServiceHandler) DeactivateUser(ctx context.Context, req *pb.DeactivateUserRequest) (*pb.DeactivateUserResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	// Ban user in Better Auth (deactivates them)
	banReq := &clients.BanUserRequest{
		UserID:    req.UserId,
		BanReason: "Deactivated by administrator",
	}

	if err := h.betterAuthClient.BanUser(ctx, banReq); err != nil {
		return nil, status.Errorf(codes.Internal, "failed to deactivate user: %v", err)
	}

	// Get updated user
	baUser, err := h.betterAuthClient.GetUser(ctx, req.UserId)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to get user: %v", err)
	}

	user := convertBetterAuthUser(baUser)

	return &pb.DeactivateUserResponse{
		User: user.ToProto(),
	}, nil
}

// BlockUser blocks a user
func (h *userServiceHandler) BlockUser(ctx context.Context, req *pb.BlockUserRequest) (*pb.BlockUserResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	reason := req.Reason
	if reason == "" {
		reason = "Blocked by administrator"
	}

	// Ban user in Better Auth
	banReq := &clients.BanUserRequest{
		UserID:    req.UserId,
		BanReason: reason,
	}

	if err := h.betterAuthClient.BanUser(ctx, banReq); err != nil {
		return nil, status.Errorf(codes.Internal, "failed to block user: %v", err)
	}

	// Get updated user
	baUser, err := h.betterAuthClient.GetUser(ctx, req.UserId)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to get user: %v", err)
	}

	user := convertBetterAuthUser(baUser)

	return &pb.BlockUserResponse{
		User: user.ToProto(),
	}, nil
}

// UnblockUser unblocks a user
func (h *userServiceHandler) UnblockUser(ctx context.Context, req *pb.UnblockUserRequest) (*pb.UnblockUserResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	// Unban user in Better Auth
	unbanReq := &clients.UnbanUserRequest{
		UserID: req.UserId,
	}

	if err := h.betterAuthClient.UnbanUser(ctx, unbanReq); err != nil {
		return nil, status.Errorf(codes.Internal, "failed to unblock user: %v", err)
	}

	// Get updated user
	baUser, err := h.betterAuthClient.GetUser(ctx, req.UserId)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to get user: %v", err)
	}

	user := convertBetterAuthUser(baUser)

	return &pb.UnblockUserResponse{
		User: user.ToProto(),
	}, nil
}

// SuspendUser suspends a user
func (h *userServiceHandler) SuspendUser(ctx context.Context, req *pb.SuspendUserRequest) (*pb.SuspendUserResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	reason := req.Reason
	if reason == "" {
		reason = "Suspended by administrator"
	}

	// Ban user in Better Auth with expiry (30 days for suspension)
	expiry := time.Now().Add(30 * 24 * time.Hour)
	banReq := &clients.BanUserRequest{
		UserID:     req.UserId,
		BanReason:  reason,
		BanExpires: &expiry,
	}

	if err := h.betterAuthClient.BanUser(ctx, banReq); err != nil {
		return nil, status.Errorf(codes.Internal, "failed to suspend user: %v", err)
	}

	// Get updated user
	baUser, err := h.betterAuthClient.GetUser(ctx, req.UserId)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to get user: %v", err)
	}

	user := convertBetterAuthUser(baUser)

	return &pb.SuspendUserResponse{
		User: user.ToProto(),
	}, nil
}

// UnsuspendUser unsuspends a user
func (h *userServiceHandler) UnsuspendUser(ctx context.Context, req *pb.UnsuspendUserRequest) (*pb.UnsuspendUserResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	// Unban user in Better Auth
	unbanReq := &clients.UnbanUserRequest{
		UserID: req.UserId,
	}

	if err := h.betterAuthClient.UnbanUser(ctx, unbanReq); err != nil {
		return nil, status.Errorf(codes.Internal, "failed to unsuspend user: %v", err)
	}

	// Get updated user
	baUser, err := h.betterAuthClient.GetUser(ctx, req.UserId)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to get user: %v", err)
	}

	user := convertBetterAuthUser(baUser)

	return &pb.UnsuspendUserResponse{
		User: user.ToProto(),
	}, nil
}

// GetUserProfile retrieves a user profile
func (h *userServiceHandler) GetUserProfile(ctx context.Context, req *pb.GetUserProfileRequest) (*pb.GetUserProfileResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	profile, err := h.userService.GetUserProfile(ctx, req.UserId)
	if err != nil {
		return nil, status.Errorf(codes.NotFound, "profile not found: %v", err)
	}

	return &pb.GetUserProfileResponse{
		Profile: profile.ToProto(),
	}, nil
}

// UpdateUserProfile updates a user profile
func (h *userServiceHandler) UpdateUserProfile(ctx context.Context, req *pb.UpdateUserProfileRequest) (*pb.UpdateUserProfileResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user ID is required")
	}

	params := users.UpdateProfileParams{
		UserID: req.UserId,
	}

	if req.Bio != nil {
		params.Bio = req.Bio
	}
	if req.Phone != nil {
		params.Phone = req.Phone
	}
	if req.Location != nil {
		params.Location = req.Location
	}
	if req.Timezone != nil {
		params.Timezone = req.Timezone
	}
	if req.Language != nil {
		params.Language = req.Language
	}
	if req.Metadata != nil {
		params.Metadata = req.Metadata.AsMap()
	}

	profile, err := h.userService.UpdateUserProfile(ctx, params)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to update profile: %v", err)
	}

	return &pb.UpdateUserProfileResponse{
		Profile: profile.ToProto(),
	}, nil
}

// CreateSession is intentionally not implemented here — sessions are created by auth-core
// during the sign-in flow. Use POST /api/auth/sign-in via auth-core.
func (h *userServiceHandler) CreateSession(_ context.Context, _ *pb.CreateSessionRequest) (*pb.CreateSessionResponse, error) {
	return nil, status.Error(codes.Unimplemented, "sessions are created by auth-core during sign-in; use POST /api/auth/sign-in")
}

// ListSessions returns all active sessions for a user via better-auth admin API
func (h *userServiceHandler) ListSessions(ctx context.Context, req *pb.ListSessionsRequest) (*pb.ListSessionsResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user_id is required")
	}

	sessions, err := h.betterAuthClient.ListSessions(ctx, &clients.ListSessionsRequest{UserID: req.UserId})
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to list sessions: %v", err)
	}

	pbSessions := make([]*pb.Session, 0, len(sessions))
	for _, s := range sessions {
		pbSession := &pb.Session{
			Id:        s.ID,
			UserId:    s.UserID,
			CreatedAt: timestamppb.New(s.CreatedAt),
			ExpiresAt: timestamppb.New(s.ExpiresAt),
		}
		if s.IPAddress != nil {
			pbSession.IpAddress = *s.IPAddress
		}
		if s.UserAgent != nil {
			pbSession.UserAgent = *s.UserAgent
		}
		pbSessions = append(pbSessions, pbSession)
	}

	return &pb.ListSessionsResponse{Sessions: pbSessions}, nil
}

// InvalidateSession revokes a specific session via better-auth admin API
func (h *userServiceHandler) InvalidateSession(ctx context.Context, req *pb.InvalidateSessionRequest) (*pb.InvalidateSessionResponse, error) {
	if req.SessionId == "" {
		return nil, status.Error(codes.InvalidArgument, "session_id is required")
	}

	if err := h.betterAuthClient.RevokeSession(ctx, &clients.RevokeSessionRequest{SessionID: req.SessionId}); err != nil {
		return nil, status.Errorf(codes.Internal, "failed to revoke session: %v", err)
	}

	return &pb.InvalidateSessionResponse{Success: true}, nil
}

// InvalidateAllSessions revokes all sessions for a user via better-auth admin API
func (h *userServiceHandler) InvalidateAllSessions(ctx context.Context, req *pb.InvalidateAllSessionsRequest) (*pb.InvalidateAllSessionsResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user_id is required")
	}

	count, err := h.betterAuthClient.RevokeUserSessions(ctx, &clients.RevokeUserSessionsRequest{UserID: req.UserId})
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to revoke user sessions: %v", err)
	}

	return &pb.InvalidateAllSessionsResponse{InvalidatedCount: int32(count)}, nil
}

// LogActivity records a user action in the activity log and publishes a NATS event
func (h *userServiceHandler) LogActivity(ctx context.Context, req *pb.LogActivityRequest) (*pb.LogActivityResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user_id is required")
	}
	if req.Action == "" {
		return nil, status.Error(codes.InvalidArgument, "action is required")
	}

	params := users.LogActivityParams{
		UserID:    req.UserId,
		Action:    req.Action,
		Resource:  req.Resource,
		IPAddress: req.IpAddress,
		UserAgent: req.UserAgent,
	}
	if req.Details != nil {
		params.Details = req.Details.AsMap()
	}

	activity, err := h.repo.LogActivity(ctx, params)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to log activity: %v", err)
	}

	// Publish NATS event (best effort)
	if h.publisher != nil {
		_ = h.publisher.PublishActivityLogged(ctx,
			req.UserId, req.Action, req.Resource, req.IpAddress, req.UserAgent, params.Details)
	}

	return &pb.LogActivityResponse{Activity: activity.ToProto()}, nil
}

// ListActivities returns paginated activity log entries for a user
func (h *userServiceHandler) ListActivities(ctx context.Context, req *pb.ListActivitiesRequest) (*pb.ListActivitiesResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user_id is required")
	}

	page := int(req.GetPagination().GetPage())
	limit := int(req.GetPagination().GetLimit())

	activities, total, err := h.repo.ListActivities(ctx, req.UserId, page, limit)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "failed to list activities: %v", err)
	}

	pbActivities := make([]*pb.Activity, 0, len(activities))
	for _, a := range activities {
		pbActivities = append(pbActivities, a.ToProto())
	}

	return &pb.ListActivitiesResponse{
		Activities: pbActivities,
		Total:      int32(total),
	}, nil
}

// AssignRole sets a user's role via better-auth admin API.
// role_id maps to better-auth role names: "user", "admin".
func (h *userServiceHandler) AssignRole(ctx context.Context, req *pb.AssignRoleRequest) (*pb.AssignRoleResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user_id is required")
	}
	if req.RoleId == "" {
		return nil, status.Error(codes.InvalidArgument, "role_id is required")
	}

	if err := h.betterAuthClient.SetRole(ctx, &clients.SetRoleRequest{
		UserID: req.UserId,
		Role:   req.RoleId,
	}); err != nil {
		return nil, status.Errorf(codes.Internal, "failed to assign role: %v", err)
	}

	return &pb.AssignRoleResponse{Success: true}, nil
}

// ListUserRoles returns the current role(s) for a user from better-auth.
// better-auth uses a single role per user; returned as a single-element list.
func (h *userServiceHandler) ListUserRoles(ctx context.Context, req *pb.ListUserRolesRequest) (*pb.ListUserRolesResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user_id is required")
	}

	baUser, err := h.betterAuthClient.GetUser(ctx, req.UserId)
	if err != nil {
		return nil, status.Errorf(codes.NotFound, "user not found: %v", err)
	}

	role := baUser.Role
	if role == "" {
		role = "user"
	}

	return &pb.ListUserRolesResponse{
		Roles: []*pb.Role{
			{Id: role, Name: role},
		},
	}, nil
}

// RemoveRole resets a user's role to "user" (the default) via better-auth admin API.
func (h *userServiceHandler) RemoveRole(ctx context.Context, req *pb.RemoveRoleRequest) (*pb.RemoveRoleResponse, error) {
	if req.UserId == "" {
		return nil, status.Error(codes.InvalidArgument, "user_id is required")
	}

	if err := h.betterAuthClient.SetRole(ctx, &clients.SetRoleRequest{
		UserID: req.UserId,
		Role:   "user",
	}); err != nil {
		return nil, status.Errorf(codes.Internal, "failed to remove role: %v", err)
	}

	return &pb.RemoveRoleResponse{Success: true}, nil
}

// RegisterDevice, ListDevices, UpdateDevice, DeactivateDevice are not implemented.
// Device tracking is handled by better-auth session metadata (user_agent / ip_address).
func (h *userServiceHandler) RegisterDevice(_ context.Context, _ *pb.RegisterDeviceRequest) (*pb.RegisterDeviceResponse, error) {
	return nil, status.Error(codes.Unimplemented, "device management not required; use session user_agent/ip tracking")
}
func (h *userServiceHandler) ListDevices(_ context.Context, _ *pb.ListDevicesRequest) (*pb.ListDevicesResponse, error) {
	return nil, status.Error(codes.Unimplemented, "device management not required; use session user_agent/ip tracking")
}
func (h *userServiceHandler) UpdateDevice(_ context.Context, _ *pb.UpdateDeviceRequest) (*pb.UpdateDeviceResponse, error) {
	return nil, status.Error(codes.Unimplemented, "device management not required; use session user_agent/ip tracking")
}
func (h *userServiceHandler) DeactivateDevice(_ context.Context, _ *pb.DeactivateDeviceRequest) (*pb.DeactivateDeviceResponse, error) {
	return nil, status.Error(codes.Unimplemented, "device management not required; use session user_agent/ip tracking")
}

// HealthCheck performs a health check
func (h *userServiceHandler) HealthCheck(ctx context.Context, req *pb.HealthCheckRequest) (*pb.HealthCheckResponse, error) {
	return &pb.HealthCheckResponse{
		Status:  pb.HealthCheckResponse_SERVING,
		Message: "User Service is healthy",
	}, nil
}
