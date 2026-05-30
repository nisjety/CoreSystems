package platform

import (
	"context"
	"net"
	"sync"
	"testing"
	"time"

	userv1 "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/proto/user/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/metadata"
	"google.golang.org/protobuf/types/known/structpb"
	"google.golang.org/protobuf/types/known/timestamppb"
)

type testUserService struct {
	userv1.UnimplementedUserServiceServer
	mu              sync.Mutex
	activities      []*userv1.Activity
	lastInternalKey string
}

func (s *testUserService) captureInternalKey(ctx context.Context) {
	md, ok := metadata.FromIncomingContext(ctx)
	if !ok {
		return
	}
	values := md.Get("x-internal-api-key")
	if len(values) == 0 {
		return
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	s.lastInternalKey = values[0]
}

func (s *testUserService) LogActivity(ctx context.Context, req *userv1.LogActivityRequest) (*userv1.LogActivityResponse, error) {
	s.captureInternalKey(ctx)
	s.mu.Lock()
	defer s.mu.Unlock()
	activity := &userv1.Activity{
		Id:        "activity-" + req.GetAction(),
		UserId:    req.GetUserId(),
		Action:    req.GetAction(),
		Resource:  req.GetResource(),
		Details:   req.GetDetails(),
		IpAddress: req.GetIpAddress(),
		UserAgent: req.GetUserAgent(),
		CreatedAt: timestamppb.New(time.Now().UTC()),
	}
	s.activities = append([]*userv1.Activity{activity}, s.activities...)
	return &userv1.LogActivityResponse{Activity: activity}, nil
}

func (s *testUserService) ListActivities(ctx context.Context, req *userv1.ListActivitiesRequest) (*userv1.ListActivitiesResponse, error) {
	s.captureInternalKey(ctx)
	s.mu.Lock()
	defer s.mu.Unlock()
	limit := int(req.GetPagination().GetLimit())
	if limit <= 0 || limit > len(s.activities) {
		limit = len(s.activities)
	}
	out := make([]*userv1.Activity, 0, limit)
	for _, item := range s.activities {
		if item.GetUserId() == req.GetUserId() {
			out = append(out, item)
			if len(out) == limit {
				break
			}
		}
	}
	return &userv1.ListActivitiesResponse{
		Activities: out,
		Total:      int32(len(out)),
	}, nil
}

func startTestUserCoreServer(t *testing.T) (string, *testUserService, func()) {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	server := grpc.NewServer()
	service := &testUserService{}
	userv1.RegisterUserServiceServer(server, service)
	go func() {
		_ = server.Serve(listener)
	}()
	return listener.Addr().String(), service, func() {
		server.GracefulStop()
		_ = listener.Close()
	}
}

func TestUserClientLogAndListActivities(t *testing.T) {
	t.Parallel()

	target, service, cleanup := startTestUserCoreServer(t)
	defer cleanup()

	client := NewUserClient(target, "test-internal-key")
	if client == nil {
		t.Fatal("client = nil, want configured client")
	}
	defer client.Close()

	if err := client.LogActivity(context.Background(), UserActivityLogParams{
		UserID:   "user-1",
		Action:   "search.created",
		Resource: "search",
		Details: map[string]interface{}{
			"jobId":   "job-1",
			"summary": "search queued",
		},
	}); err != nil {
		t.Fatalf("LogActivity: %v", err)
	}

	activities, total, err := client.ListActivities(context.Background(), "user-1", 10)
	if err != nil {
		t.Fatalf("ListActivities: %v", err)
	}
	if total != 1 || len(activities) != 1 {
		t.Fatalf("activities = %d/%d, want 1/1", len(activities), total)
	}
	if activities[0].Action != "search.created" || activities[0].Resource != "search" {
		t.Fatalf("activity = %+v, want search.created/search", activities[0])
	}

	service.mu.Lock()
	lastInternalKey := service.lastInternalKey
	service.mu.Unlock()
	if lastInternalKey != "test-internal-key" {
		t.Fatalf("internal key metadata = %q, want %q", lastInternalKey, "test-internal-key")
	}
}

func TestControlPlaneServiceListUserActivitiesUsesCache(t *testing.T) {
	t.Parallel()

	target, service, cleanup := startTestUserCoreServer(t)
	defer cleanup()

	details, _ := structpb.NewStruct(map[string]interface{}{"summary": "search queued"})
	service.activities = []*userv1.Activity{{
		Id:        "activity-1",
		UserId:    "user-1",
		Action:    "search.created",
		Resource:  "search",
		Details:   details,
		CreatedAt: timestamppb.New(time.Now().UTC()),
	}}

	controlPlane := NewControlPlaneService(ControlPlaneConfig{
		UserBaseURL:    target,
		InternalAPIKey: "test-internal-key",
		CacheTTL:       time.Minute,
	})
	defer controlPlane.Close()

	first, total, err := controlPlane.ListUserActivities(context.Background(), "user-1", 10)
	if err != nil {
		t.Fatalf("first ListUserActivities: %v", err)
	}
	if total != 1 || len(first) != 1 {
		t.Fatalf("first activities = %d/%d, want 1/1", len(first), total)
	}

	service.mu.Lock()
	service.activities = nil
	service.mu.Unlock()

	second, total, err := controlPlane.ListUserActivities(context.Background(), "user-1", 10)
	if err != nil {
		t.Fatalf("second ListUserActivities: %v", err)
	}
	if total != 1 || len(second) != 1 {
		t.Fatalf("cached activities = %d/%d, want 1/1", len(second), total)
	}
}
