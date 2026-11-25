// Package clients holds org-core's outbound calls to other planes. Each is narrow and read-only and
// carries its own credential. None reads another plane's database.
package clients

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/netip"
	"strconv"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/servicetoken"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
)

// OrgCoreServiceID is org-core's service principal. session-core admits GetThreadRetention only from
// "service:org-core" (THREAD_RETENTION_READ_SERVICE), so every token this package presents is minted for it.
const OrgCoreServiceID = "org-core"

const (
	// sessionCoreAudience is the Auth Core audience of the Model Plane session-core.
	sessionCoreAudience = "session-core"
	// threadRetentionReadScope is the one scope GetThreadRetention checks (THREAD_RETENTION_READ_SCOPE).
	threadRetentionReadScope = "session:retention:read"
	// threadRetentionReason is what Auth Core audits for each mint.
	threadRetentionReason = "legal-hold thread receipt check"
)

// ThreadReceipt is session-core's answer for one thread. Found is false when session-core holds no
// receipt for the thread. NothingRetained is true only for a receipt that says Zero Data Retention or
// a waived run (DEC-32). Only a found receipt can say that nothing was retained.
type ThreadReceipt struct {
	Found           bool
	NothingRetained bool
}

// threadReceiptSource is the one call this client makes to session-core. The gRPC binding implements
// it over SessionRetentionService.GetThreadRetention (F-f10-02); tests implement it with a fake.
type threadReceiptSource interface {
	GetThreadRetention(ctx context.Context, orgID, threadID string) (ThreadReceipt, error)
}

// TokenSource mints a session-core token bound to one organisation. servicetoken.Provider is the
// production implementation. Each read asks for the token of the organisation it reads, because
// session-core refuses a token whose organisation differs from the request's.
type TokenSource interface {
	Token(ctx context.Context, orgID string) (string, error)
}

// threadReceiptTimeout bounds one receipt read, the token mint included. A thread hold waits for it, so a
// stalled session-core cannot hold a legal-hold request open. A read that times out is an unknown receipt,
// and the hold is placed.
const threadReceiptTimeout = 5 * time.Second

// SessionThreadRetentionClient answers whether session-core retained any content for a thread. It
// keeps no answer between calls, and it never guesses: a transport or permission error is returned,
// and the caller places the hold.
type SessionThreadRetentionClient struct {
	source    threadReceiptSource
	closeConn func() error
}

func newSessionThreadRetentionClient(source threadReceiptSource, closeConn func() error) *SessionThreadRetentionClient {
	return &SessionThreadRetentionClient{source: source, closeConn: closeConn}
}

// NewSessionCoreTokenSource mints tokens for serviceID through Auth Core's service-token exchange, presenting
// the service API key as the credential. The tokens are for the session-core audience and carry the one
// retention read scope. Every field is required, and an error names the missing field, never the credential.
func NewSessionCoreTokenSource(authCoreURL, serviceID, credential string) (TokenSource, error) {
	provider, err := servicetoken.New(servicetoken.Config{
		AuthCoreURL: authCoreURL,
		ServiceID:   serviceID,
		Credential:  credential,
		Audience:    sessionCoreAudience,
		Scopes:      []string{threadRetentionReadScope},
		Reason:      threadRetentionReason,
	})
	if err != nil {
		return nil, err
	}
	return provider, nil
}

// NewSessionThreadRetentionClient dials session-core's SessionRetentionService at addr, which must be host:port
// on the private network. Each read presents a token from tokens for the organisation it reads. An unset
// address or token source is refused, and so is an address off the private network. The boot switch turns
// that refusal into a refusal to start.
func NewSessionThreadRetentionClient(addr string, tokens TokenSource) (*SessionThreadRetentionClient, error) {
	addr = strings.TrimSpace(addr)
	if addr == "" {
		return nil, errors.New("the session-core thread receipt address is unset")
	}
	if tokens == nil {
		return nil, errors.New("the session-core thread receipt token source is unset")
	}
	if !isPrivateNetworkTarget(addr) {
		return nil, errors.New("the session-core thread receipt address must be host:port on the private network")
	}
	// session-core serves gRPC on its private listener without TLS (its crate has no TLS listener), and
	// sandbox-manager dials it the same way. The plaintext hop carries a token, so isPrivateNetworkTarget
	// keeps it off public networks.
	conn, err := grpc.NewClient(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return nil, fmt.Errorf("dial session-core thread receipt: %w", err)
	}
	source := grpcThreadReceiptSource{client: mpv1.NewSessionRetentionServiceClient(conn), tokens: tokens}
	return newSessionThreadRetentionClient(source, conn.Close), nil
}

// grpcThreadReceiptSource is the gRPC binding of threadReceiptSource. Each read takes the token for the
// organisation it reads, then makes one GetThreadRetention call with that token as the authorization
// metadata. The answer carries two booleans only.
type grpcThreadReceiptSource struct {
	client mpv1.SessionRetentionServiceClient
	tokens TokenSource
}

// GetThreadRetention reads one thread's receipt from session-core. It sends the two ids and the token, and
// nothing else. A token that cannot be minted stops the read before any call reaches session-core.
func (s grpcThreadReceiptSource) GetThreadRetention(ctx context.Context, orgID, threadID string) (ThreadReceipt, error) {
	ctx, cancel := context.WithTimeout(ctx, threadReceiptTimeout)
	defer cancel()
	token, err := s.tokens.Token(ctx, orgID)
	if err != nil {
		return ThreadReceipt{}, fmt.Errorf("session-core token for the thread receipt read: %w", err)
	}
	if strings.TrimSpace(token) == "" {
		return ThreadReceipt{}, errors.New("session-core token for the thread receipt read is empty")
	}
	ctx = metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+token)
	response, err := s.client.GetThreadRetention(ctx, &mpv1.GetThreadRetentionRequest{OrgId: orgID, ThreadId: threadID})
	if err != nil {
		return ThreadReceipt{}, err
	}
	return ThreadReceipt{Found: response.GetFound(), NothingRetained: response.GetNothingRetained()}, nil
}

// NothingRetained is true only when session-core holds a receipt for the thread that says nothing was
// retained. Without a receipt the thread is not proven empty: the answer is false with no error, and
// the hold is placed as for any retained thread.
func (c *SessionThreadRetentionClient) NothingRetained(ctx context.Context, orgID, threadID string) (bool, error) {
	if c == nil || c.source == nil {
		return false, errors.New("the session-core thread receipt client is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	threadID = strings.TrimSpace(threadID)
	if orgID == "" || threadID == "" {
		return false, errors.New("an organization and a thread are required to read the thread receipt")
	}
	receipt, err := c.source.GetThreadRetention(ctx, orgID, threadID)
	if err != nil {
		return false, fmt.Errorf("read session-core thread receipt: %w", err)
	}
	if !receipt.Found {
		return false, nil
	}
	return receipt.NothingRetained, nil
}

// Close releases the connection when the client owns one.
func (c *SessionThreadRetentionClient) Close() error {
	if c == nil || c.closeConn == nil {
		return nil
	}
	return c.closeConn()
}

// isPrivateNetworkTarget is true for a host:port that stays on the private network: a loopback, private or
// link-local IP literal, or a name, which resolves on the stack's own network. A public IP literal is refused,
// so a live Auth Core token cannot leave in plaintext for the internet because an address was misconfigured.
// A name is the operator's to keep private. The port must be a number from 1 to 65535.
func isPrivateNetworkTarget(target string) bool {
	host, port, err := net.SplitHostPort(strings.TrimSpace(target))
	if err != nil || host == "" {
		return false
	}
	if number, err := strconv.Atoi(port); err != nil || number < 1 || number > 65535 {
		return false
	}
	// netip keeps an IPv6 zone ("fe80::1%eth0"), so a zoned public literal is classed as an IP and refused,
	// not taken for a name. Unmap turns "::ffff:8.8.8.8" into the IPv4 address it carries.
	addr, err := netip.ParseAddr(host)
	if err != nil {
		return true
	}
	addr = addr.Unmap()
	return addr.IsLoopback() || addr.IsPrivate() || addr.IsLinkLocalUnicast()
}
