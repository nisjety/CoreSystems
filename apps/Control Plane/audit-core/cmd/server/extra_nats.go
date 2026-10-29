package main

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"net/url"
	"regexp"
	"strings"
	"sync"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/api"
	metricsserver "github.com/triodelab/controlplane/audit-core/internal/metrics"
	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/subscriber"
)

const (
	maxExtraNATSBuses    = 16
	extraNATSRetryPeriod = time.Second
)

// JetStream consumer names reject token separators ('.', '*', '>') and
// whitespace, so the configured identity is restricted before startup.
var busNamePattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_-]{1,63}$`)

type extraNATSBus struct {
	Name     string `json:"name"`
	Plane    string `json:"plane"`
	URL      string `json:"url"`
	User     string `json:"user,omitempty"`
	Password string `json:"password,omitempty"`
	Token    string `json:"token,omitempty"`
}

type natsCredential struct {
	User     string
	Password string
	Token    string
}

func parseExtraNATSBuses(raw string, allowTokenFallback bool) ([]extraNATSBus, error) {
	if strings.TrimSpace(raw) == "" {
		return nil, nil
	}
	decoder := json.NewDecoder(strings.NewReader(raw))
	decoder.DisallowUnknownFields()
	var candidates []extraNATSBus
	if err := decoder.Decode(&candidates); err != nil {
		return nil, fmt.Errorf("decode AUDIT_EXTRA_NATS_BUSES: %w", err)
	}
	var trailer any
	if err := decoder.Decode(&trailer); err != io.EOF {
		if err == nil {
			return nil, fmt.Errorf("AUDIT_EXTRA_NATS_BUSES contains trailing JSON")
		}
		return nil, fmt.Errorf("decode AUDIT_EXTRA_NATS_BUSES trailer: %w", err)
	}
	if len(candidates) > maxExtraNATSBuses {
		return nil, fmt.Errorf("AUDIT_EXTRA_NATS_BUSES exceeds %d entries", maxExtraNATSBuses)
	}

	seen := make(map[string]struct{}, len(candidates))
	seenTokens := make(map[[sha256.Size]byte]struct{}, len(candidates))
	validated := make([]extraNATSBus, 0, len(candidates))
	for _, candidate := range candidates {
		bus := extraNATSBus{
			Name:     strings.TrimSpace(candidate.Name),
			Plane:    strings.TrimSpace(candidate.Plane),
			URL:      strings.TrimSpace(candidate.URL),
			User:     strings.TrimSpace(candidate.User),
			Password: strings.TrimSpace(candidate.Password),
			Token:    strings.TrimSpace(candidate.Token),
		}
		if !busNamePattern.MatchString(bus.Name) {
			return nil, fmt.Errorf("AUDIT_EXTRA_NATS_BUSES contains an invalid bus name")
		}
		if _, exists := seen[bus.Name]; exists {
			return nil, fmt.Errorf("AUDIT_EXTRA_NATS_BUSES contains a duplicate bus name")
		}
		seen[bus.Name] = struct{}{}
		if !busNamePattern.MatchString(bus.Plane) {
			return nil, fmt.Errorf("AUDIT_EXTRA_NATS_BUSES bus %q has an invalid plane authority", bus.Name)
		}
		if err := validateNATSURL(bus.URL); err != nil {
			return nil, fmt.Errorf("AUDIT_EXTRA_NATS_BUSES bus %q: %w", bus.Name, err)
		}
		credential, err := selectNATSCredential(bus.User, bus.Password, bus.Token, allowTokenFallback)
		if err != nil {
			return nil, fmt.Errorf("AUDIT_EXTRA_NATS_BUSES bus %q: %w", bus.Name, err)
		}
		bus.User, bus.Password, bus.Token = credential.User, credential.Password, credential.Token
		secret := bus.Password
		if secret == "" {
			secret = bus.Token
		}
		tokenDigest := sha256.Sum256([]byte(secret))
		if _, exists := seenTokens[tokenDigest]; exists {
			return nil, fmt.Errorf("AUDIT_EXTRA_NATS_BUSES must use a distinct credential per bus")
		}
		seenTokens[tokenDigest] = struct{}{}
		validated = append(validated, bus)
	}
	return validated, nil
}

func selectNATSCredential(user, password, token string, allowTokenFallback bool) (natsCredential, error) {
	user = strings.TrimSpace(user)
	password = strings.TrimSpace(password)
	token = strings.TrimSpace(token)
	if user != "" || password != "" {
		if !busNamePattern.MatchString(user) {
			return natsCredential{}, fmt.Errorf("NATS user is invalid")
		}
		if !secureNATSToken(password) {
			return natsCredential{}, fmt.Errorf("NATS password does not meet the production credential policy")
		}
		return natsCredential{User: user, Password: password}, nil
	}
	if !allowTokenFallback {
		return natsCredential{}, fmt.Errorf("scoped NATS user/password is required; token fallback is disabled")
	}
	if !secureNATSToken(token) {
		return natsCredential{}, fmt.Errorf("NATS token does not meet the migration credential policy")
	}
	return natsCredential{Token: token}, nil
}

func validateNATSURL(raw string) error {
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Hostname() == "" {
		return fmt.Errorf("invalid NATS URL")
	}
	switch parsed.Scheme {
	case "nats", "tls", "ws", "wss":
	default:
		return fmt.Errorf("unsupported NATS URL scheme")
	}
	if parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" || (parsed.Path != "" && parsed.Path != "/") {
		return fmt.Errorf("NATS URL must not embed credentials, query, fragment, or path")
	}
	return nil
}

func secureNATSToken(token string) bool {
	lower := strings.ToLower(strings.TrimSpace(token))
	return len(token) >= 32 &&
		!strings.HasPrefix(lower, "test") &&
		!strings.HasPrefix(lower, "placeholder") &&
		!strings.HasPrefix(lower, "change-me") &&
		!strings.HasPrefix(lower, "replace-with")
}

func connectNamedNATS(bus extraNATSBus) (*nats.Conn, error) {
	opts := []nats.Option{
		nats.Name("audit-core-aggregator-" + bus.Name),
		nats.CustomInboxPrefix(auditInboxPrefix(bus.Plane)),
		nats.RetryOnFailedConnect(true),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(extraNATSRetryPeriod),
		nats.DisconnectErrHandler(func(_ *nats.Conn, err error) {
			metricsserver.SetNATSConnected(bus.Name, false)
			log.Warn().Err(err).Str("bus", bus.Name).Msg("extra nats bus disconnected")
		}),
		nats.ReconnectHandler(func(_ *nats.Conn) {
			metricsserver.SetNATSConnected(bus.Name, true)
			log.Info().Str("bus", bus.Name).Msg("extra nats bus reconnected")
		}),
		nats.ClosedHandler(func(_ *nats.Conn) {
			metricsserver.SetNATSConnected(bus.Name, false)
			log.Warn().Str("bus", bus.Name).Msg("extra nats bus connection closed")
		}),
	}
	if bus.User != "" {
		opts = append(opts, nats.UserInfo(bus.User, bus.Password))
	} else {
		opts = append(opts, nats.Token(bus.Token))
	}
	return nats.Connect(bus.URL, opts...)
}

func auditInboxPrefix(plane string) string {
	return "_INBOX.AUDIT_" + strings.ToUpper(strings.ReplaceAll(strings.TrimSpace(plane), "-", "_"))
}

type managedExtraNATSBus struct {
	config extraNATSBus
	store  *store.Store

	mu              sync.RWMutex
	connection      *nats.Conn
	subscriber      *subscriber.Subscriber
	subscriberReady bool
}

func startManagedExtraNATSBus(ctx context.Context, config extraNATSBus, eventStore *store.Store) *managedExtraNATSBus {
	managed := &managedExtraNATSBus{config: config, store: eventStore}
	go managed.run(ctx)
	return managed
}

func (m *managedExtraNATSBus) run(ctx context.Context) {
	ticker := time.NewTicker(extraNATSRetryPeriod)
	defer ticker.Stop()
	for {
		m.ensureStarted(ctx)
		select {
		case <-ctx.Done():
			m.Close()
			return
		case <-ticker.C:
		}
	}
}

func (m *managedExtraNATSBus) ensureStarted(ctx context.Context) {
	m.mu.RLock()
	connection := m.connection
	ready := m.subscriberReady
	m.mu.RUnlock()
	if ready && connection != nil && !connection.IsClosed() {
		return
	}
	if ready {
		m.mu.Lock()
		if m.connection == connection && (connection == nil || connection.IsClosed()) {
			m.connection = nil
			m.subscriber = nil
			m.subscriberReady = false
		}
		m.mu.Unlock()
		connection = nil
	}
	if connection == nil || connection.IsClosed() {
		connected, err := connectNamedNATS(m.config)
		if err != nil {
			log.Warn().Err(err).Str("bus", m.config.Name).Msg("extra nats bus connect failed; will retry")
			return
		}
		m.mu.Lock()
		m.connection = connected
		m.mu.Unlock()
		connection = connected
	}
	if !connection.IsConnected() {
		metricsserver.SetNATSConnected(m.config.Name, false)
		return
	}

	busSubscriber := subscriber.New(connection, m.store, m.config.Name, m.config.Plane)
	if err := busSubscriber.Start(ctx); err != nil {
		log.Warn().Err(err).Str("bus", m.config.Name).Msg("extra nats subscriber start failed; will retry")
		connection.Close()
		m.mu.Lock()
		if m.connection == connection {
			m.connection = nil
		}
		m.mu.Unlock()
		return
	}
	m.mu.Lock()
	m.subscriber = busSubscriber
	m.subscriberReady = true
	m.mu.Unlock()
	metricsserver.SetNATSConnected(m.config.Name, true)
	log.Info().Str("bus", m.config.Name).Msg("extra plane audit consumers ready")
}

func (m *managedExtraNATSBus) Readiness() api.NATSBusReadiness {
	m.mu.RLock()
	connection := m.connection
	busSubscriber := m.subscriber
	subscriberReady := m.subscriberReady
	m.mu.RUnlock()
	connected := connection != nil && connection.IsConnected()
	metricsserver.SetNATSConnected(m.config.Name, connected)
	return subscriberReadiness(m.config.Name, connected, subscriberReady, busSubscriber)
}

func (m *managedExtraNATSBus) Close() {
	m.mu.Lock()
	connection := m.connection
	m.connection = nil
	m.subscriber = nil
	m.subscriberReady = false
	m.mu.Unlock()
	if connection != nil {
		_ = connection.Drain()
	}
	metricsserver.SetNATSConnected(m.config.Name, false)
}

func subscriberReadiness(name string, connected, started bool, busSubscriber *subscriber.Subscriber) api.NATSBusReadiness {
	result := api.NATSBusReadiness{Name: name, Connected: connected, SubscriberReady: started}
	if !connected || !started || busSubscriber == nil {
		return result
	}
	health := busSubscriber.Health()
	result.Audit = consumerReadiness(health.Audit)
	result.Usage = consumerReadiness(health.Usage)
	return result
}

func consumerReadiness(health subscriber.ConsumerHealth) api.ConsumerReadiness {
	return api.ConsumerReadiness{
		Consumer:    health.Consumer,
		Ready:       health.Ready,
		Pending:     health.Pending,
		AckPending:  health.AckPending,
		Redelivered: health.Redelivered,
		LastAckAt:   health.LastAckAt,
	}
}
