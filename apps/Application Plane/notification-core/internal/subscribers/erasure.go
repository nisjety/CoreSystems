package subscribers

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"
)

// providerEraseTimeout bounds one organization's provider-side erasure. A pass that times out is retried by the erasure
// consumer, and each pass removes what the one before left, so a large organization converges across retries.
const providerEraseTimeout = 5 * time.Minute

// errProviderEraseOrganizationRequired refuses a provider-side erasure that names no organization, so an empty id can
// never widen into a sweep of every membership.
var errProviderEraseOrganizationRequired = errors.New("organization_id is required to erase provider identities")

// errProviderEraseUserRequired refuses a provider-side user erasure that names no user, for the same reason.
var errProviderEraseUserRequired = errors.New("user_id is required to erase provider identities")

// MembershipLister lists the memberships an erasure reads, active and removed: every membership of one organization, or
// every membership of one user across every organization. The erasure needs the removed ones as well: removing a membership
// does not delete its provider subscriber, so the subscriber is still there to erase.
type MembershipLister interface {
	ListMembershipsForOrganization(ctx context.Context, organizationID string) ([]Membership, error)
	ListMembershipsForUser(ctx context.Context, userID string) ([]Membership, error)
}

// ProviderIdentityClient is the provider surface an organization erasure needs. The Novu adapter satisfies it, and a test
// can supply a fake. Both calls are idempotent: a message or subscriber that is already gone is not an error.
type ProviderIdentityClient interface {
	// DeleteSubscriberMessages removes every message the provider holds for one subscriber, on every channel, and returns
	// how many this call removed.
	DeleteSubscriberMessages(ctx context.Context, subscriberID string) (int, error)
	// DeleteSubscriber removes one provider subscriber and reports whether this call removed it.
	DeleteSubscriber(ctx context.Context, subscriberID string) (bool, error)
}

// ProviderEraser removes an organization's or a user's provider-side identities. Each membership owns one provider
// subscriber, and its id is derived from the organization and the user together. A subscriber therefore belongs to exactly
// one organization, so erasing one organization cannot touch another organization's identity for the same user. Erasing a
// user removes that user's subscriber in each organization, one membership at a time.
type ProviderEraser struct {
	memberships MembershipLister
	provider    ProviderIdentityClient
}

// NewProviderEraser wires the eraser over the membership store and the provider client.
func NewProviderEraser(memberships MembershipLister, provider ProviderIdentityClient) *ProviderEraser {
	return &ProviderEraser{memberships: memberships, provider: provider}
}

// PurgeOrganization deletes, for every membership of the organization, the provider's messages for that membership's
// subscriber, and then the subscriber itself. Messages go first so that listing still finds them. It returns the number
// of messages and subscribers it removed, never their content.
//
// It must run before the local purge removes the memberships. A membership row is the only record of which provider
// subscriber belongs to the organization, so the local purge would otherwise leave the provider records orphaned.
func (e *ProviderEraser) PurgeOrganization(ctx context.Context, organizationID string) (map[string]int64, error) {
	if e == nil || e.memberships == nil || e.provider == nil {
		return nil, errors.New("provider erasure is not configured")
	}
	organizationID = strings.TrimSpace(organizationID)
	if organizationID == "" {
		return nil, errProviderEraseOrganizationRequired
	}
	ctx, cancel := context.WithTimeout(ctx, providerEraseTimeout)
	defer cancel()

	memberships, err := e.memberships.ListMembershipsForOrganization(ctx, organizationID)
	if err != nil {
		return nil, fmt.Errorf("list organization memberships: %w", err)
	}

	counts := newEraseCounts()
	for _, membership := range memberships {
		// The lister is scoped to the organization. A row for any other organization is refused here, not erased.
		if membership.OrganizationID != organizationID {
			continue
		}
		if err := e.eraseMembership(ctx, membership, counts); err != nil {
			return counts, err
		}
	}
	return counts, nil
}

// PurgeUser deletes, for every membership of the user in every organization, the provider's messages for that membership's
// subscriber and then the subscriber itself, as PurgeOrganization does for one organization. It is the per-user erasure, and
// it runs before the local purge for the same reason: the membership rows are the only record of which provider subscriber
// belongs to which organization. The user is erased across organizations because the Auth erasure behind it is global.
func (e *ProviderEraser) PurgeUser(ctx context.Context, userID string) (map[string]int64, error) {
	if e == nil || e.memberships == nil || e.provider == nil {
		return nil, errors.New("provider erasure is not configured")
	}
	userID = strings.TrimSpace(userID)
	if userID == "" {
		return nil, errProviderEraseUserRequired
	}
	ctx, cancel := context.WithTimeout(ctx, providerEraseTimeout)
	defer cancel()

	memberships, err := e.memberships.ListMembershipsForUser(ctx, userID)
	if err != nil {
		return nil, fmt.Errorf("list user memberships: %w", err)
	}

	counts := newEraseCounts()
	for _, membership := range memberships {
		// The lister is scoped to the user. A row for any other user is refused here, not erased.
		if membership.UserID != userID {
			continue
		}
		if err := e.eraseMembership(ctx, membership, counts); err != nil {
			return counts, err
		}
	}
	return counts, nil
}

// newEraseCounts returns the counts an erasure reports, with both keys present even when nothing was removed.
func newEraseCounts() map[string]int64 {
	return map[string]int64{"novu_subscriber_messages": 0, "novu_subscribers": 0}
}

// eraseMembership deletes one membership's provider messages and then its subscriber, and adds what it removed to counts. A
// membership with no provider identity has no subscriber to erase, so it is skipped rather than sent to the provider with an
// empty id, which would address the collection instead of one subscriber.
func (e *ProviderEraser) eraseMembership(ctx context.Context, membership Membership, counts map[string]int64) error {
	subscriberID := strings.TrimSpace(membership.ProviderSubscriberID)
	if subscriberID == "" {
		return nil
	}
	messages, err := e.provider.DeleteSubscriberMessages(ctx, subscriberID)
	counts["novu_subscriber_messages"] += int64(messages)
	if err != nil {
		return fmt.Errorf("delete provider messages: %w", err)
	}
	removed, err := e.provider.DeleteSubscriber(ctx, subscriberID)
	if err != nil {
		return fmt.Errorf("delete provider subscriber: %w", err)
	}
	if removed {
		counts["novu_subscribers"]++
	}
	return nil
}
