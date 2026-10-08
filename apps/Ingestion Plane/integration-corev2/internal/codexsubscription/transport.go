package codexsubscription

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/rs/zerolog"
)

// TransportKind names how the broker reaches the ChatGPT backend for
// inference.
type TransportKind string

const (
	// TransportDirect sends Responses API requests straight to the ChatGPT
	// Codex backend with the connection's ChatGPT OAuth token. It is opt-in
	// until OpenAI confirms it for a hosted, multi-user product.
	TransportDirect TransportKind = "direct"
	// TransportAppServer spawns the pinned, hardened Codex app-server per call
	// (default).
	TransportAppServer TransportKind = "app-server"
)

// ParseTransportKind accepts "", "direct", "app-server" (case-insensitive,
// trimmed); "" means app-server. Anything else is an error.
func ParseTransportKind(raw string) (TransportKind, error) {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "", string(TransportAppServer):
		return TransportAppServer, nil
	case string(TransportDirect):
		return TransportDirect, nil
	}
	return "", fmt.Errorf("unsupported Codex subscription transport %q: want direct or app-server", strings.TrimSpace(raw))
}

// Transport runs exactly one inference turn for one connection home.
type Transport interface {
	Kind() TransportKind
	Invoke(ctx context.Context, codeHome string, req InvokeRequest) (InvokeResponse, error)
	InvokeStream(ctx context.Context, codeHome string, req InvokeRequest, onDelta func(string) error) (InvokeResponse, error)
}

// Authenticator owns sign-in and sign-out for a connection home.
type Authenticator interface {
	BeginDeviceLogin(ctx context.Context, codeHome string) (LoginProcess, DeviceCode, error)
	Logout(ctx context.Context, codeHome string) error
}

// CredentialLocker serializes work on one connection's credentials.
type CredentialLocker interface {
	WithExclusive(ctx context.Context, codeHome string, fn func() error) error
}

// RunnerOptions selects and configures the inference transport. Login and
// logout always use the Codex CLI device-code flow in phase 1.
type RunnerOptions struct {
	Transport        TransportKind
	AppServerCommand string
	// AppServerHardened applies the §9 hardening flags to every spawned
	// Codex process. The runtime config defaults it to true.
	AppServerHardened bool
	// AppServerThreadEffort sends the turn's reasoning effort at thread/start
	// so the startup prewarm can match the turn. The runtime config defaults
	// it to true.
	AppServerThreadEffort bool
	Direct                DirectConfig
	// RefreshSkew is how long before access-token expiry the broker
	// refreshes; defaults to 10m.
	RefreshSkew time.Duration
	// Logger receives content-free lines only.
	Logger zerolog.Logger
}

// NewRunner builds one shared credential store, the Codex CLI process runner
// (always: it is the phase-1 Authenticator), and the selected Transport.
func NewRunner(opts RunnerOptions) (Runner, error) {
	kind, err := ParseTransportKind(string(opts.Transport))
	if err != nil {
		return nil, err
	}
	client := opts.Direct.HTTPClient
	if client == nil {
		client = defaultDirectHTTPClient(opts.Direct.InvocationTimeout)
	}
	originator := strings.TrimSpace(opts.Direct.Originator)
	if originator == "" {
		originator = defaultOriginator
	}
	creds := newCredentialStore(credentialStoreConfig{
		TokenURL:   opts.Direct.TokenURL,
		HTTPClient: client,
		Skew:       opts.RefreshSkew,
		UserAgent:  directUserAgent(),
		Originator: originator,
		Logger:     opts.Logger,
	})
	process := NewProcessRunner(opts.AppServerCommand,
		WithCredentials(creds),
		WithHardening(opts.AppServerHardened),
		WithThreadEffort(opts.AppServerThreadEffort),
		WithLogger(opts.Logger),
	)
	var transport Transport = process
	if kind == TransportDirect {
		direct := opts.Direct
		direct.HTTPClient = client
		direct.Originator = originator
		// One logger for the whole runner: RunnerOptions.Logger wins over
		// DirectConfig.Logger here.
		direct.Logger = opts.Logger
		transport, err = NewDirectTransport(direct, creds)
		if err != nil {
			return nil, err
		}
	}
	return NewRunnerWithTransport(process, transport, creds), nil
}

// NewRunnerWithTransport composes an Authenticator and a Transport around a
// credential lock. A nil locker runs Logout unlocked.
func NewRunnerWithTransport(auth Authenticator, transport Transport, creds CredentialLocker) Runner {
	return &composedRunner{auth: auth, transport: transport, creds: creds}
}

// composedRunner is the Runner the Manager sees: inference goes to the
// transport, sign-in and sign-out to the authenticator.
type composedRunner struct {
	auth      Authenticator
	transport Transport
	creds     CredentialLocker
}

func (r *composedRunner) BeginDeviceLogin(ctx context.Context, codeHome string) (LoginProcess, DeviceCode, error) {
	if r.auth == nil {
		return nil, DeviceCode{}, errors.New("codex subscription authenticator is not configured")
	}
	return r.auth.BeginDeviceLogin(ctx, codeHome)
}

func (r *composedRunner) Invoke(ctx context.Context, codeHome string, request InvokeRequest) (InvokeResponse, error) {
	return r.transport.Invoke(ctx, codeHome, request)
}

func (r *composedRunner) InvokeStream(ctx context.Context, codeHome string, request InvokeRequest, onDelta func(string) error) (InvokeResponse, error) {
	return r.transport.InvokeStream(ctx, codeHome, request, onDelta)
}

// Logout holds the connection's credential lock, so a refresh in flight can
// never write auth.json back after logout deleted it.
func (r *composedRunner) Logout(ctx context.Context, codeHome string) error {
	if r.auth == nil {
		return errors.New("codex subscription authenticator is not configured")
	}
	if r.creds == nil {
		return r.auth.Logout(ctx, codeHome)
	}
	return r.creds.WithExclusive(ctx, codeHome, func() error { return r.auth.Logout(ctx, codeHome) })
}

func (r *composedRunner) ReadRateLimits(ctx context.Context, codeHome string) (RateLimitReport, error) {
	reader, ok := r.auth.(RateLimitReader)
	if !ok {
		return RateLimitReport{}, errors.New("Codex allowance reader is unavailable")
	}
	var report RateLimitReport
	var readErr error
	read := func() error { report, readErr = reader.ReadRateLimits(ctx, codeHome); return readErr }
	if r.creds == nil {
		return report, read()
	}
	lockedRead := read
	if lockedReader, ok := r.auth.(interface {
		readRateLimitsFromAppServer(context.Context, string) (RateLimitReport, error)
	}); ok {
		// ProcessRunner.ReadRateLimits also ensures credentials. Since the
		// composed runner already holds the lock, bypass that second acquisition.
		lockedRead = func() error {
			report, readErr = lockedReader.readRateLimitsFromAppServer(ctx, codeHome)
			return readErr
		}
	}
	err := r.creds.WithExclusive(ctx, codeHome, lockedRead)
	if err != nil {
		return RateLimitReport{}, err
	}
	return report, readErr
}

func (r *composedRunner) TransportKind() TransportKind { return r.transport.Kind() }
