package grpc

import (
	"context"
	"time"

	healthv1 "github.com/coresystem/integration-gateway/api/proto/health/v1"
	"github.com/coresystem/integration-gateway/internal/health"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// HealthServiceServer implements the gRPC HealthService
type HealthServiceServer struct {
	healthv1.UnimplementedHealthServiceServer
	checker   *health.HealthChecker
	dbChecker *health.DatabaseChecker
}

// NewHealthServiceServer creates a new gRPC health service server
func NewHealthServiceServer(checker *health.HealthChecker, dbChecker *health.DatabaseChecker) *HealthServiceServer {
	return &HealthServiceServer{
		checker:   checker,
		dbChecker: dbChecker,
	}
}

// Check performs a comprehensive health check
func (s *HealthServiceServer) Check(ctx context.Context, req *healthv1.HealthCheckRequest) (*healthv1.HealthCheckResponse, error) {
	healthStatus := s.checker.Check(ctx)

	resp := &healthv1.HealthCheckResponse{
		Status:        convertHealthStatus(healthStatus.Status),
		Message:       "",
		Timestamp:     timestamppb.New(healthStatus.Timestamp),
		UptimeSeconds: int64(healthStatus.Uptime.Seconds()),
	}

	// Add PostgreSQL health if available
	if pg, ok := healthStatus.Database["postgres"].(health.PostgreSQLHealth); ok {
		resp.Postgresql = &healthv1.DatabaseHealth{
			Status:         convertHealthStatus(pg.Status),
			Name:           "postgresql",
			ResponseTimeMs: pg.ResponseTime.Milliseconds(),
			Error:          pg.Error,
		}
	}

	// Add Redis health if available
	if redis, ok := healthStatus.Database["redis"].(health.RedisHealth); ok {
		resp.Redis = &healthv1.DatabaseHealth{
			Status:         convertHealthStatus(redis.Status),
			Name:           "redis",
			ResponseTimeMs: redis.ResponseTime.Milliseconds(),
			Error:          redis.Error,
		}
	}

	return resp, nil
}

// Liveness checks if the service is alive (Kubernetes liveness probe)
func (s *HealthServiceServer) Liveness(ctx context.Context, req *healthv1.LivenessRequest) (*healthv1.LivenessResponse, error) {
	alive := s.checker.Liveness()

	return &healthv1.LivenessResponse{
		Alive:     alive,
		Timestamp: timestamppb.New(time.Now()),
	}, nil
}

// Readiness checks if the service is ready to handle requests (Kubernetes readiness probe)
func (s *HealthServiceServer) Readiness(ctx context.Context, req *healthv1.ReadinessRequest) (*healthv1.ReadinessResponse, error) {
	ready := s.checker.Readiness(ctx)
	message := ""
	if !ready {
		message = "Service not ready - database unavailable"
	}

	return &healthv1.ReadinessResponse{
		Ready:     ready,
		Message:   message,
		Timestamp: timestamppb.New(time.Now()),
	}, nil
}

// CheckPostgreSQL checks PostgreSQL database health
func (s *HealthServiceServer) CheckPostgreSQL(ctx context.Context, req *healthv1.PostgreSQLHealthRequest) (*healthv1.PostgreSQLHealthResponse, error) {
	pgHealth := s.dbChecker.CheckPostgreSQL(ctx)

	return &healthv1.PostgreSQLHealthResponse{
		Status:            convertHealthStatus(pgHealth.Status),
		Version:           pgHealth.Version,
		ActiveConnections: int32(pgHealth.Connections),
		MaxConnections:    int32(pgHealth.MaxConnections),
		DatabaseSizeBytes: 0, // DatabaseSize is a string, would need parsing
		ResponseTimeMs:    pgHealth.ResponseTime.Milliseconds(),
		Error:             pgHealth.Error,
	}, nil
}

// CheckRedis checks Redis cache health
func (s *HealthServiceServer) CheckRedis(ctx context.Context, req *healthv1.RedisHealthRequest) (*healthv1.RedisHealthResponse, error) {
	redisHealth := s.dbChecker.CheckRedis(ctx)

	return &healthv1.RedisHealthResponse{
		Status:           convertHealthStatus(redisHealth.Status),
		Version:          redisHealth.Version,
		ConnectedClients: int32(redisHealth.ConnectedClients),
		UsedMemoryBytes:  int64(redisHealth.UsedMemory),
		ResponseTimeMs:   redisHealth.ResponseTime.Milliseconds(),
		Error:            redisHealth.Error,
	}, nil
}

// convertHealthStatus converts internal health status to proto enum
func convertHealthStatus(status string) healthv1.HealthStatus {
	switch status {
	case "healthy":
		return healthv1.HealthStatus_HEALTH_STATUS_HEALTHY
	case "degraded":
		return healthv1.HealthStatus_HEALTH_STATUS_DEGRADED
	case "unhealthy":
		return healthv1.HealthStatus_HEALTH_STATUS_UNHEALTHY
	default:
		return healthv1.HealthStatus_HEALTH_STATUS_UNSPECIFIED
	}
}
