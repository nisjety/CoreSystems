package platform

import (
	"context"
	"fmt"
	"net/url"
	"strings"
	"time"

	userv1 "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/proto/user/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
	"google.golang.org/protobuf/types/known/structpb"
)

type UserActivity struct {
	ID        string                 `json:"id"`
	UserID    string                 `json:"userId"`
	Action    string                 `json:"action"`
	Resource  string                 `json:"resource"`
	Details   map[string]interface{} `json:"details,omitempty"`
	IPAddress string                 `json:"ipAddress,omitempty"`
	UserAgent string                 `json:"userAgent,omitempty"`
	CreatedAt time.Time              `json:"createdAt"`
}

type UserActivityLogParams struct {
	UserID    string
	Action    string
	Resource  string
	Details   map[string]interface{}
	IPAddress string
	UserAgent string
}

type UserClient struct {
	target         string
	internalAPIKey string
	conn           *grpc.ClientConn
	client         userv1.UserServiceClient
}

func NewUserClient(target, internalAPIKey string) *UserClient {
	target = normalizeGRPCTarget(target)
	if strings.TrimSpace(target) == "" {
		return nil
	}
	conn, err := grpc.Dial(target, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return nil
	}
	return &UserClient{
		target:         target,
		internalAPIKey: strings.TrimSpace(internalAPIKey),
		conn:           conn,
		client:         userv1.NewUserServiceClient(conn),
	}
}

func (c *UserClient) Close() error {
	if c == nil || c.conn == nil {
		return nil
	}
	return c.conn.Close()
}

func (c *UserClient) outgoingContext(ctx context.Context) context.Context {
	if c == nil || c.internalAPIKey == "" {
		return ctx
	}
	return metadata.AppendToOutgoingContext(ctx, "x-internal-api-key", c.internalAPIKey)
}

func (c *UserClient) LogActivity(ctx context.Context, params UserActivityLogParams) error {
	if c == nil || c.client == nil {
		return fmt.Errorf("user-core not configured")
	}
	if strings.TrimSpace(params.UserID) == "" || strings.TrimSpace(params.Action) == "" {
		return fmt.Errorf("user id and action are required")
	}
	details, err := structpb.NewStruct(params.Details)
	if err != nil {
		return fmt.Errorf("build activity details: %w", err)
	}
	_, err = c.client.LogActivity(c.outgoingContext(ctx), &userv1.LogActivityRequest{
		UserId:    params.UserID,
		Action:    params.Action,
		Resource:  params.Resource,
		Details:   details,
		IpAddress: params.IPAddress,
		UserAgent: params.UserAgent,
	})
	return err
}

func (c *UserClient) ListActivities(ctx context.Context, userID string, limit int) ([]UserActivity, int, error) {
	if c == nil || c.client == nil {
		return nil, 0, fmt.Errorf("user-core not configured")
	}
	if strings.TrimSpace(userID) == "" {
		return nil, 0, fmt.Errorf("user id is required")
	}
	if limit <= 0 {
		limit = 25
	}
	resp, err := c.client.ListActivities(c.outgoingContext(ctx), &userv1.ListActivitiesRequest{
		UserId: userID,
		Pagination: &userv1.Pagination{
			Page:  1,
			Limit: int32(limit),
		},
	})
	if err != nil {
		return nil, 0, err
	}
	activities := make([]UserActivity, 0, len(resp.GetActivities()))
	for _, item := range resp.GetActivities() {
		activities = append(activities, fromProtoActivity(item))
	}
	return activities, int(resp.GetTotal()), nil
}

func (s *ControlPlaneService) ListUserActivities(ctx context.Context, userID string, limit int) ([]UserActivity, int, error) {
	if s == nil || strings.TrimSpace(userID) == "" {
		return nil, 0, fmt.Errorf("user activity lookup is not configured")
	}
	if limit <= 0 {
		limit = 25
	}
	cacheKey := fmt.Sprintf("user-activities:%s:%d", userID, limit)
	if cached, ok := s.cache.Get(cacheKey); ok {
		if payload, ok := cached.(userActivityCacheEntry); ok {
			return cloneUserActivities(payload.Activities), payload.Total, nil
		}
	}
	if s.user == nil {
		return nil, 0, fmt.Errorf("user client is not configured")
	}
	activities, total, err := s.user.ListActivities(ctx, userID, limit)
	if err != nil {
		return nil, 0, err
	}
	s.cache.Set(cacheKey, userActivityCacheEntry{Activities: cloneUserActivities(activities), Total: total}, s.ttl)
	return cloneUserActivities(activities), total, nil
}

func (s *ControlPlaneService) LogUserActivity(ctx context.Context, params UserActivityLogParams) error {
	if s == nil {
		return fmt.Errorf("control plane service is not configured")
	}
	if s.user == nil {
		return nil
	}
	if err := s.user.LogActivity(ctx, params); err != nil {
		return err
	}
	for _, key := range []string{
		fmt.Sprintf("user-activities:%s:%d", params.UserID, 25),
		fmt.Sprintf("user-activities:%s:%d", params.UserID, 100),
	} {
		s.cache.Delete(key)
	}
	return nil
}

func (s *ControlPlaneService) Close() error {
	if s == nil || s.user == nil {
		return nil
	}
	return s.user.Close()
}

type userActivityCacheEntry struct {
	Activities []UserActivity
	Total      int
}

func fromProtoActivity(activity *userv1.Activity) UserActivity {
	if activity == nil {
		return UserActivity{}
	}
	details := map[string]interface{}{}
	if activity.GetDetails() != nil {
		details = activity.GetDetails().AsMap()
	}
	createdAt := time.Time{}
	if activity.GetCreatedAt() != nil {
		createdAt = activity.GetCreatedAt().AsTime()
	}
	return UserActivity{
		ID:        activity.GetId(),
		UserID:    activity.GetUserId(),
		Action:    activity.GetAction(),
		Resource:  activity.GetResource(),
		Details:   details,
		IPAddress: activity.GetIpAddress(),
		UserAgent: activity.GetUserAgent(),
		CreatedAt: createdAt,
	}
}

func cloneUserActivities(input []UserActivity) []UserActivity {
	if input == nil {
		return nil
	}
	output := make([]UserActivity, 0, len(input))
	for _, item := range input {
		cloned := item
		cloned.Details = cloneAnyMap(item.Details)
		output = append(output, cloned)
	}
	return output
}

func normalizeGRPCTarget(target string) string {
	target = strings.TrimSpace(target)
	if target == "" {
		return ""
	}
	if !strings.Contains(target, "://") {
		return target
	}
	parsed, err := url.Parse(target)
	if err != nil {
		return target
	}
	if parsed.Host != "" {
		return parsed.Host
	}
	return strings.TrimPrefix(target, parsed.Scheme+"://")
}
