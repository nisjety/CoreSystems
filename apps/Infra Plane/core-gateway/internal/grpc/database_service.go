package grpc

import (
	"context"

	databasev1 "github.com/coresystem/integration-gateway/api/proto/database/v1"
	"github.com/coresystem/integration-gateway/internal/health"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// DatabaseServiceServer implements the gRPC DatabaseService
type DatabaseServiceServer struct {
	databasev1.UnimplementedDatabaseServiceServer
	checker *health.DatabaseChecker
}

// NewDatabaseServiceServer creates a new gRPC database service server
func NewDatabaseServiceServer(checker *health.DatabaseChecker) *DatabaseServiceServer {
	return &DatabaseServiceServer{
		checker: checker,
	}
}

// CheckPostgreSQL performs PostgreSQL health check
func (s *DatabaseServiceServer) CheckPostgreSQL(ctx context.Context, req *databasev1.CheckPostgreSQLRequest) (*databasev1.CheckPostgreSQLResponse, error) {
	pgHealth := s.checker.CheckPostgreSQL(ctx)

	return &databasev1.CheckPostgreSQLResponse{
		Status:            convertToDatabaseStatus(pgHealth.Status),
		Version:           pgHealth.Version,
		ActiveConnections: int32(pgHealth.Connections),
		MaxConnections:    int32(pgHealth.MaxConnections),
		DatabaseSizeBytes: parseDatabaseSize(pgHealth.DatabaseSize),
		ResponseTimeMs:    pgHealth.ResponseTime.Milliseconds(),
		CheckedAt:         timestamppb.Now(),
		Error:             pgHealth.Error,
	}, nil
}

// CheckRedis performs Redis health check
func (s *DatabaseServiceServer) CheckRedis(ctx context.Context, req *databasev1.CheckRedisRequest) (*databasev1.CheckRedisResponse, error) {
	redisHealth := s.checker.CheckRedis(ctx)

	return &databasev1.CheckRedisResponse{
		Status:           convertToDatabaseStatus(redisHealth.Status),
		Version:          redisHealth.Version,
		ConnectedClients: int32(redisHealth.ConnectedClients),
		UsedMemoryBytes:  int64(redisHealth.UsedMemory),
		MaxMemoryBytes:   0, // Not provided by current implementation
		ResponseTimeMs:   redisHealth.ResponseTime.Milliseconds(),
		CheckedAt:        timestamppb.Now(),
		Error:            redisHealth.Error,
	}, nil
}

// GetPostgreSQLStats returns detailed PostgreSQL statistics
func (s *DatabaseServiceServer) GetPostgreSQLStats(ctx context.Context, req *databasev1.GetPostgreSQLStatsRequest) (*databasev1.GetPostgreSQLStatsResponse, error) {
	pgHealth := s.checker.CheckPostgreSQL(ctx)

	// For now, return basic stats from health check
	// This can be enhanced with more detailed stats queries
	return &databasev1.GetPostgreSQLStatsResponse{
		Version:           pgHealth.Version,
		TotalConnections:  int32(pgHealth.Connections),
		ActiveConnections: int32(pgHealth.Connections),
		IdleConnections:   0, // Not provided
		MaxConnections:    int32(pgHealth.MaxConnections),
		DatabaseSizeBytes: parseDatabaseSize(pgHealth.DatabaseSize),
		TableCount:        0, // Not provided
		IndexCount:        0, // Not provided
		CacheHitRatio:     0, // Not provided
	}, nil
}

// GetRedisStats returns detailed Redis statistics
func (s *DatabaseServiceServer) GetRedisStats(ctx context.Context, req *databasev1.GetRedisStatsRequest) (*databasev1.GetRedisStatsResponse, error) {
	redisHealth := s.checker.CheckRedis(ctx)

	// For now, return basic stats from health check
	// This can be enhanced with INFO command parsing
	return &databasev1.GetRedisStatsResponse{
		Version:          redisHealth.Version,
		Mode:             "standalone", // Default assumption
		ConnectedClients: int32(redisHealth.ConnectedClients),
		BlockedClients:   0,
		UsedMemoryBytes:  int64(redisHealth.UsedMemory),
	}, nil
}

// GetConnectionPoolStats returns connection pool information
func (s *DatabaseServiceServer) GetConnectionPoolStats(ctx context.Context, req *databasev1.GetConnectionPoolStatsRequest) (*databasev1.GetConnectionPoolStatsResponse, error) {
	// This is a placeholder - actual implementation would query pool stats
	return &databasev1.GetConnectionPoolStatsResponse{
		PoolName:          req.PoolName,
		TotalConnections:  0,
		IdleConnections:   0,
		ActiveConnections: 0,
		MaxConnections:    0,
	}, nil
}

// Helper functions

func convertToDatabaseStatus(status string) databasev1.DatabaseStatus {
	switch status {
	case "healthy":
		return databasev1.DatabaseStatus_DATABASE_STATUS_HEALTHY
	case "degraded":
		return databasev1.DatabaseStatus_DATABASE_STATUS_DEGRADED
	case "unhealthy":
		return databasev1.DatabaseStatus_DATABASE_STATUS_UNHEALTHY
	case "unavailable":
		return databasev1.DatabaseStatus_DATABASE_STATUS_UNAVAILABLE
	default:
		return databasev1.DatabaseStatus_DATABASE_STATUS_UNSPECIFIED
	}
}

func parseDatabaseSize(sizeStr string) int64 {
	// Parse size string like "1.5 MB" to bytes
	// This is a simplified implementation
	// A full implementation would parse units properly
	return 0
}
