package orchestration

import (
	"log/slog"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/natsx"
)

// New constructs the orchestration Service and its gRPC Handlers wired
// together with a shared NATS publisher and an upstream session-core
// client. Callers register the returned Handlers on a *grpc.Server.
func New(pub natsx.RawPublisher, logger *slog.Logger, client mpv1.OrchestrationCoreServiceClient) (*Service, *Handlers) {
	svc := NewService(pub, logger)
	return svc, NewHandlers(svc, client)
}
