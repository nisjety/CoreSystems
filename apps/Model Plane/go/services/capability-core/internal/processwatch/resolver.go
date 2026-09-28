package processwatch

import (
	"context"

	"google.golang.org/grpc/metadata"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/watch"
)

// Resolver proves at CREATE time that a requested process exists in the
// requesting Space, and says where a watch on it should start reading.
//
// # Why creation checks this when the poll already does
//
// Nothing leaks without it — the adapter refuses a foreign process on every
// poll. But a watch created against another Space's process would sit ACTIVE,
// occupy one of the Space's eight slots, and fail silently forever: the person
// would be told their watch exists and would never hear from it. Refusing at
// create is the difference between an error and a lie.
//
// # Why it does not start at zero
//
// A watch created on a process that has been running for an hour must not
// replay that hour. The person asked to be told what happens NEXT, and dumping
// an hour of backlog into the room as "events" would bury the thing they were
// actually waiting for. So the starting cursor is the process's current
// position.
//
// The cost, stated: output produced between the process starting and the watch
// being created is never matched. That is the intended behaviour, not a gap —
// a watch is not a search.
type Resolver struct {
	client registryClient
	tokens TokenProvider
}

// NewResolver constructs the create-time source check.
func NewResolver(client registryClient, tokens TokenProvider) (*Resolver, error) {
	if client == nil || tokens == nil {
		return nil, errResolverConfig
	}
	return &Resolver{client: client, tokens: tokens}, nil
}

// Resolve implements watch.SourceResolver.
//
// Returns [watch.ErrSourceGone] for a missing process, a process in another
// Space, and one whose content sits above the requester's audience ceiling —
// one indistinguishable answer, so a create cannot become an oracle for which
// process ids exist in an organization.
func (r *Resolver) Resolve(ctx context.Context, w watch.Watch) (int64, error) {
	token, err := r.tokens.Token(ctx, w.OrgID)
	if err != nil {
		return 0, err
	}
	authorized := metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+token)

	resolved, err := r.client.GetProcess(authorized, &mpv1.GetProcessRequest{ProcessId: w.SourceRef})
	if err != nil {
		return 0, watch.ErrSourceGone
	}
	process := resolved.GetProcess()
	if process == nil || process.GetSpaceId() != w.SpaceRef {
		return 0, watch.ErrSourceGone
	}
	if ceiling := w.Authority.RecipientAudienceRevision; ceiling > 0 &&
		process.GetRecipientAudienceRevision() > ceiling {
		return 0, watch.ErrSourceGone
	}
	// A process that has already finished can still be watched — the watch will
	// drain whatever output remains and report the outcome. Refusing here would
	// make "tell me how that ended" impossible for anything that ended a second
	// before the person asked.
	return process.GetNextSeq() - 1, nil
}

var errResolverConfig = configError("the process-output resolver requires a registry client and a token provider")

type configError string

func (e configError) Error() string { return string(e) }
