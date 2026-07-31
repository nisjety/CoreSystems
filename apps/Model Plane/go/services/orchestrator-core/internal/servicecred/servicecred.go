// Package servicecred gives orchestrator-core's Temporal activities a
// credential of their own for outbound calls to sibling Model Plane services.
//
// # The gap this closes
//
// orchestrator-core reaches its siblings through one gRPC client factory used
// by two callers with completely different credential situations:
//
//   - The PROXY path. An inbound RPC (run-event streaming, approval decisions)
//     arrives with the caller's verified bearer in gRPC metadata, and the
//     factory's interceptor copies that inbound metadata onto the outbound call.
//     This works and must keep working untouched.
//
//   - The ACTIVITY path. A Temporal activity's context comes from the SDK
//     worker, not from a gRPC server handler, so `metadata.FromIncomingContext`
//     reports nothing to forward. Every activity therefore dialed its sibling
//     with NO credential at all and the sibling answered Unauthenticated. Only
//     `codes.Unavailable` is treated as non-fatal in the activities, so an
//     Unauthenticated reply is a hard activity failure: the durable tier could
//     start workflows but no activity that touched a sibling could do real work.
//
// This package supplies the missing half — a minted, audience-bound service
// token attached only when there is no inbound credential to forward.
//
// # Why the org comes from the request
//
// Auth Core mints a token bound to one organization and session-core enforces
// an exact match against it, so the tenant must be known before minting. Most
// request messages carry it (`GetOrgId`), and that is the preferred source
// because it is the same value the callee will authorize against — reading it
// from the message makes a mismatch impossible. [WithOrg] covers the RPCs whose
// proto has no org field (capability-core's skill promotion), where the org
// lives in the activity input instead.
//
// A missing org is returned as an error naming the audience rather than
// defaulting to anything. A guessed tenant mints a token that authenticates and
// is then refused on every call, which reads as a puzzling permission problem;
// a named error points straight at the caller that forgot to supply it.
//
// # What is deliberately NOT wired
//
// execution-core has no minter and must not get one. Its `ExecuteStep`
// authorizes with `caller.authorize(&req.org_id, Some(&req.user_id))` — the
// request's user must equal the CALLER's own user — and resolves run ownership
// by that same user id. A minted service token's subject is the service itself
// (auth-core's `issueInternalToken` sets `userId: principal.subject` and accepts
// no delegation field), so it can only ever prove "this service, in this org".
// Satisfying execution-core from a background workflow would mean letting a
// service act with a user's privileges on that user's run, which is a policy
// decision about tool execution, not a wiring gap. Left explicitly unwired.
package servicecred

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// Plane audience slugs. These must equal auth-core's `exactAudience()` slugs —
// it throws unless the configured audience string matches the path segment, so a
// typo here surfaces at mint time rather than as a subtly wrong token.
const (
	AudienceSessionCore    = "session-core"
	AudienceInferenceCore  = "inference-core"
	AudienceCapabilityCore = "capability-core"
	AudienceLettaBridge    = "letta-bridge"
)

// metadataAuthorization is the gRPC metadata key every Model Plane service reads
// its caller credential from.
const metadataAuthorization = "authorization"

// Minter mints and caches short-lived tokens for exactly one plane audience.
// It is the subset of *servicetoken.Provider this package needs, so the
// interceptor can be tested without an HTTP issuer.
type Minter interface {
	// Token returns a live bearer for orgID.
	Token(ctx context.Context, orgID string) (string, error)
	// Invalidate drops the cached token for orgID so the next Token call mints
	// a fresh one. Used as the single-retry backstop on Unauthenticated.
	Invalidate(orgID string)
	// Audience reports the plane audience, used only in error text.
	Audience() string
}

type orgContextKey struct{}

// WithOrg tags ctx with the tenant to mint for.
//
// Needed only for RPCs whose request message has no org field — capability-core's
// ValidateSkillBundle / CheckSkillPromotion / PromoteSkill carry a skill id and
// scopes but no org_id, so their tenancy exists only in the activity input.
func WithOrg(ctx context.Context, orgID string) context.Context {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return ctx
	}
	return context.WithValue(ctx, orgContextKey{}, orgID)
}

// OrgFrom reports the tenant tagged by [WithOrg], or "" when none is set.
func OrgFrom(ctx context.Context) string {
	org, _ := ctx.Value(orgContextKey{}).(string)
	return org
}

// orgScoped is satisfied by every generated request message with an `org_id`
// field. Reading the org off the message keeps the minted token's tenant and the
// tenant the callee authorizes identical by construction.
type orgScoped interface{ GetOrgId() string }

// hasForwardableCredential reports whether ctx carries an inbound caller
// credential to forward.
//
// The check is for a non-empty `authorization` specifically, not merely for the
// presence of inbound metadata: an inbound RPC can arrive with metadata that has
// no bearer, and forwarding that empty set would send an uncredentialed call
// while looking like the proxy path worked.
func hasForwardableCredential(ctx context.Context) bool {
	md, ok := metadata.FromIncomingContext(ctx)
	if !ok {
		return false
	}
	for _, value := range md.Get(metadataAuthorization) {
		if strings.TrimSpace(value) != "" {
			return true
		}
	}
	return false
}

// forwardInbound copies inbound metadata onto the outbound context — the
// unchanged proxy path.
func forwardInbound(ctx context.Context) context.Context {
	if md, ok := metadata.FromIncomingContext(ctx); ok {
		return metadata.NewOutgoingContext(ctx, md)
	}
	return ctx
}

// resolveOrg picks the tenant to mint for: an explicit [WithOrg] tag wins,
// otherwise the request message's own org_id.
//
// WithOrg is checked first so a caller can be explicit for a message type that
// happens to have an org field but leaves it empty.
func resolveOrg(ctx context.Context, req any) string {
	if org := OrgFrom(ctx); org != "" {
		return org
	}
	if scoped, ok := req.(orgScoped); ok {
		return strings.TrimSpace(scoped.GetOrgId())
	}
	return ""
}

// UnaryInterceptor authenticates outbound unary calls on one connection.
//
// Order of precedence, and why:
//
//  1. An inbound credential is forwarded unchanged. The proxy path is acting on
//     a real user's behalf and that user's authority — never the service's — is
//     what the callee must see.
//  2. Otherwise a token is minted for the resolved org. This is the activity
//     path.
//
// A nil minter degrades to plain forwarding rather than failing the call, so a
// deployment that has not configured minting behaves exactly as it did before
// this package existed instead of breaking the working proxy path.
func UnaryInterceptor(minter Minter, logger *slog.Logger) grpc.UnaryClientInterceptor {
	if logger == nil {
		logger = slog.Default()
	}
	return func(
		ctx context.Context,
		method string,
		req, reply any,
		cc *grpc.ClientConn,
		invoker grpc.UnaryInvoker,
		opts ...grpc.CallOption,
	) error {
		if hasForwardableCredential(ctx) || minter == nil {
			return invoker(forwardInbound(ctx), method, req, reply, cc, opts...)
		}

		org := resolveOrg(ctx, req)
		if org == "" {
			return status.Errorf(
				codes.InvalidArgument,
				"servicecred: %s call %s has no organization to mint a token for "+
					"(request carries no org_id and none was set with WithOrg)",
				minter.Audience(), method,
			)
		}

		token, err := minter.Token(ctx, org)
		if err != nil {
			return status.Errorf(
				codes.Unauthenticated,
				"servicecred: mint %s token: %v", minter.Audience(), err,
			)
		}

		err = invoker(withBearer(ctx, token), method, req, reply, cc, opts...)
		if status.Code(err) != codes.Unauthenticated {
			return err
		}

		// One retry on a fresh token. The refresh margin normally prevents
		// expiry mid-flight, so reaching here means the cached token was
		// rejected for a reason the margin cannot cover — a credential rotated
		// underneath this process, a re-keyed issuer, or clock skew. Retrying
		// once self-heals those instead of wedging until restart; a second
		// Unauthenticated is a real authorization answer and is returned.
		logger.Warn("service token rejected, re-minting once",
			"audience", minter.Audience(), "method", method, "org_id", org)
		minter.Invalidate(org)
		token, mintErr := minter.Token(ctx, org)
		if mintErr != nil {
			return errors.Join(err, fmt.Errorf("re-mint %s token: %w", minter.Audience(), mintErr))
		}
		return invoker(withBearer(ctx, token), method, req, reply, cc, opts...)
	}
}

// StreamInterceptor forwards an inbound credential onto outbound streams.
//
// It does NOT mint. A stream's first message is not available when the stream is
// created, so there is no org to mint for at that point, and no activity opens a
// stream — every activity RPC is unary. Minting here would be dead code guarding
// a case that cannot arise, so streams keep exactly the proxy behavior they had.
func StreamInterceptor() grpc.StreamClientInterceptor {
	return func(
		ctx context.Context,
		desc *grpc.StreamDesc,
		cc *grpc.ClientConn,
		method string,
		streamer grpc.Streamer,
		opts ...grpc.CallOption,
	) (grpc.ClientStream, error) {
		return streamer(forwardInbound(ctx), desc, cc, method, opts...)
	}
}

// withBearer sets the outbound authorization header.
//
// It builds on whatever metadata the context already carries rather than
// starting from an empty set, so headers that are not credentials — request ids,
// trace context — survive. Set (not Append) is used so the re-mint retry
// replaces the rejected bearer instead of sending two on one call.
func withBearer(ctx context.Context, token string) context.Context {
	md, ok := metadata.FromOutgoingContext(ctx)
	if ok {
		md = md.Copy()
	} else if inbound, hasInbound := metadata.FromIncomingContext(ctx); hasInbound {
		md = inbound.Copy()
	} else {
		md = metadata.MD{}
	}
	md.Set(metadataAuthorization, "Bearer "+token)
	return metadata.NewOutgoingContext(ctx, md)
}
