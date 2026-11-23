package server

import (
	"context"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

// This file is the gRPC face of F-f10-08. It carries the retention_erase_gen
// build tag only because go/gen has not been regenerated for the
// EraseThreadWorkspaceForRetention RPC in sandboxes.proto yet. Once the lead has
// regenerated go/gen/model_plane/v1 (sandboxes.pb.go and sandboxes_grpc.pb.go),
// delete the //go:build line above (and the one in
// retention_erase_grpc_test.go) so the handler is part of every build.

type EraseThreadWorkspaceForRetentionRequest = mpv1.EraseThreadWorkspaceForRetentionRequest
type EraseThreadWorkspaceForRetentionResponse = mpv1.EraseThreadWorkspaceForRetentionResponse

// EraseThreadWorkspaceForRetention erases a deleted thread's workspace rows and
// blobs for session-core's retention sweeper (F-f10-08, owner D-f10-7). Every
// check lives in eraseThreadWorkspaceForRetention; this only unwraps the request
// and answers with the row count, never a name or a path.
func (s *Server) EraseThreadWorkspaceForRetention(ctx context.Context, req *EraseThreadWorkspaceForRetentionRequest) (*EraseThreadWorkspaceForRetentionResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "EraseThreadWorkspaceForRetention")))
	files, err := s.eraseThreadWorkspaceForRetention(ctx, req.GetOrgId(), req.GetThreadId(), req.GetOwnerId())
	if err != nil {
		return nil, err
	}
	return &EraseThreadWorkspaceForRetentionResponse{FilesDeleted: files}, nil
}
