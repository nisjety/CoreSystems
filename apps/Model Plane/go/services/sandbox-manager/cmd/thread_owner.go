package main

import (
	"fmt"
	"log/slog"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/servicetoken"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadowner"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// threadOwnerCheckEnabled reads SANDBOX_MANAGER_THREAD_OWNER_CHECK strictly: only the
// literal "true" enables it (O-13).
func threadOwnerCheckEnabled(value string) bool {
	return value == "true"
}

const (
	defaultAuthCoreURL   = "http://auth-core:3011"
	defaultServiceID     = "sandbox-manager"
	defaultSessionCore   = "session-core:9091"
	threadOwnerTokenWhy  = "sandbox-manager verifies thread ownership before it serves or erases a thread workspace"
	serviceCredentialEnv = "SANDBOX_MANAGER_SERVICE_API_KEY"
)

// newExistenceTransport builds the wire half of session-core's batch thread existence
// read. It is nil until the additive ThreadsExist RPC is generated: the orphan sweep
// needs it, and without it the sweep stays off and the ownership check is unaffected.
// The file that sets it carries the generated client call and nothing else.
var newExistenceTransport func(grpc.ClientConnInterface) threadowner.ExistenceTransport

// threadOwnerWiring is what the ownership check needs from session-core: the
// resolver the server asks, and the pieces the orphan sweep reuses.
type threadOwnerWiring struct {
	resolver threadowner.Resolver
	tokens   threadowner.TokenSource
	conn     *grpc.ClientConn
}

// threadOwnerFromEnv builds the O-13 resolver: a service-token provider for audience
// session-core with the one narrow scope, a session-core connection, and the 30
// second positive cache over them. A switch that is on and cannot be built is an
// error the caller stops startup on, never a silent fall back to no check.
func threadOwnerFromEnv(getenv func(string) string) (*threadOwnerWiring, error) {
	authCoreURL := strings.TrimSpace(getenv("AUTH_CORE_URL"))
	if authCoreURL == "" {
		authCoreURL = defaultAuthCoreURL
	}
	serviceID := strings.TrimSpace(getenv("SANDBOX_MANAGER_SERVICE_ID"))
	if serviceID == "" {
		serviceID = defaultServiceID
	}
	sessionCoreAddr := strings.TrimSpace(getenv("SESSION_CORE_ADDR"))
	if sessionCoreAddr == "" {
		sessionCoreAddr = defaultSessionCore
	}
	tokens, err := servicetoken.New(servicetoken.Config{
		AuthCoreURL: authCoreURL,
		ServiceID:   serviceID,
		Credential:  getenv(serviceCredentialEnv),
		Audience:    "session-core",
		Scopes:      []string{threadowner.Scope},
		Reason:      threadOwnerTokenWhy,
	})
	if err != nil {
		return nil, fmt.Errorf("%w (set %s and AUTH_CORE_URL)", err, serviceCredentialEnv)
	}
	conn, err := grpc.NewClient(sessionCoreAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return nil, fmt.Errorf("dial session-core: %w", err)
	}
	direct, err := threadowner.NewSessionCore(mpv1.NewRunServiceClient(conn), tokens)
	if err != nil {
		_ = conn.Close()
		return nil, err
	}
	cached, err := threadowner.NewCache(direct, threadowner.DefaultCacheTTL)
	if err != nil {
		_ = conn.Close()
		return nil, err
	}
	return &threadOwnerWiring{resolver: cached, tokens: tokens, conn: conn}, nil
}

// orphanSweepFor adds the O-13 orphan pass to the retention sweeper when the
// existence transport is built, and leaves the sweeper as it was otherwise (one
// warning). The grace window is read here so a malformed value stops startup.
func orphanSweepFor(sweeper *threadworkspace.Sweeper, rows *threadworkspace.Eraser, w *threadOwnerWiring, getenv func(string) string) (*threadworkspace.Sweeper, error) {
	grace, err := threadworkspace.OrphanGraceFromEnv(getenv)
	if err != nil {
		return nil, err
	}
	if newExistenceTransport == nil {
		slog.Warn("orphan thread workspace sweep is off: this build has no session-core thread existence read, so a deleted thread's workspace waits for retention or the service erase")
		return sweeper, nil
	}
	existence, err := threadowner.NewSessionCoreExistence(newExistenceTransport(w.conn), w.tokens)
	if err != nil {
		return nil, err
	}
	slog.Info("orphan thread workspace sweep enabled", "grace", grace.String())
	return sweeper.WithOrphanSweep(rows, existence, grace), nil
}
