package session

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/go-rod/rod"
	"github.com/go-rod/rod/lib/proto"
	"github.com/go-rod/stealth"
	"github.com/google/uuid"
	"github.com/rs/zerolog/log"
)

// BrowserSession holds a long-lived Rod page and metadata.
type BrowserSession struct {
	ID        string    `json:"id"`
	URL       string    `json:"url"`
	CreatedAt time.Time `json:"created_at"`
	ExpiresAt time.Time `json:"expires_at"`
	LastUsed  time.Time `json:"last_used"`
	StepCount int       `json:"step_count"`
	Profile   string    `json:"profile,omitempty"`
	OrgID     string    `json:"org_id,omitempty"`
	UserID    string    `json:"user_id,omitempty"`

	page         *rod.Page
	mu           sync.Mutex
	closed       bool
	cleanup      func() // releases browser pool slot
	profileStore ProfileStore
}

// Manager manages interactive browser sessions with TTL and inactivity timeout.
type Manager struct {
	sessions      map[string]*BrowserSession
	mu            sync.RWMutex
	pool          PageProvider
	sessionTTL    time.Duration
	inactivityTTL time.Duration
	maxSessions   int
	profileStore  ProfileStore
	stop          chan struct{}
}

// PageProvider is the interface for acquiring a browser page from the pool.
type PageProvider interface {
	GetPage(ctx context.Context) (*rod.Page, func(), error)
}

// Config holds session manager configuration.
type Config struct {
	SessionTTL     time.Duration // Max lifetime per session (default 10m)
	InactivityTTL  time.Duration // Close after inactivity (default 5m)
	MaxSessions    int           // Max concurrent sessions (default 20)
	ReaperInterval time.Duration // How often to check for expired sessions (default 30s)
	ProfileStore   ProfileStore
}

func DefaultConfig() Config {
	return Config{
		SessionTTL:     10 * time.Minute,
		InactivityTTL:  5 * time.Minute,
		MaxSessions:    20,
		ReaperInterval: 30 * time.Second,
	}
}

func NewManager(pool PageProvider, cfg Config) *Manager {
	if cfg.SessionTTL <= 0 {
		cfg.SessionTTL = 10 * time.Minute
	}
	if cfg.InactivityTTL <= 0 {
		cfg.InactivityTTL = 5 * time.Minute
	}
	if cfg.MaxSessions <= 0 {
		cfg.MaxSessions = 20
	}
	if cfg.ReaperInterval <= 0 {
		cfg.ReaperInterval = 30 * time.Second
	}

	m := &Manager{
		sessions:      make(map[string]*BrowserSession),
		pool:          pool,
		sessionTTL:    cfg.SessionTTL,
		inactivityTTL: cfg.InactivityTTL,
		maxSessions:   cfg.MaxSessions,
		profileStore:  cfg.ProfileStore,
		stop:          make(chan struct{}),
	}
	if m.profileStore == nil {
		m.profileStore = NewInMemoryProfileStore()
	}

	go m.reaper(cfg.ReaperInterval)

	return m
}

// Create opens a new browser page, navigates to URL, and returns a session ID.
func (m *Manager) Create(ctx context.Context, targetURL string, opts *CreateOptions) (*BrowserSession, error) {
	m.mu.Lock()
	if len(m.sessions) >= m.maxSessions {
		m.mu.Unlock()
		return nil, fmt.Errorf("session limit reached (%d)", m.maxSessions)
	}
	m.mu.Unlock()

	page, cleanup, err := m.pool.GetPage(ctx)
	if err != nil {
		return nil, fmt.Errorf("acquire browser page: %w", err)
	}

	// Inject stealth on new documents.
	if _, err := page.EvalOnNewDocument(stealth.JS); err != nil {
		cleanup()
		return nil, fmt.Errorf("inject stealth: %w", err)
	}

	// Apply viewport if provided.
	if opts != nil && opts.Viewport != nil {
		_ = proto.EmulationSetDeviceMetricsOverride{
			Width:             opts.Viewport.Width,
			Height:            opts.Viewport.Height,
			DeviceScaleFactor: opts.Viewport.DeviceScaleFactor,
			Mobile:            opts.Mobile,
		}.Call(page)
	}

	if err := page.Navigate(targetURL); err != nil {
		cleanup()
		return nil, fmt.Errorf("navigate: %w", err)
	}
	if err := page.WaitLoad(); err != nil {
		cleanup()
		return nil, fmt.Errorf("wait load: %w", err)
	}

	profileName := ""
	if opts != nil {
		profileName = strings.TrimSpace(opts.Profile)
	}
	if profileName != "" {
		if err := m.applyProfile(ctx, page, targetURL, profileName); err != nil && !strings.Contains(strings.ToLower(err.Error()), "not found") {
			log.Warn().Err(err).Str("profile", profileName).Msg("failed to apply browser profile")
		}
	}

	now := time.Now()
	orgID := ""
	userID := ""
	if opts != nil {
		orgID = strings.TrimSpace(opts.OrgID)
		userID = strings.TrimSpace(opts.UserID)
	}
	sess := &BrowserSession{
		ID:           uuid.NewString(),
		URL:          targetURL,
		Profile:      profileName,
		OrgID:        orgID,
		UserID:       userID,
		CreatedAt:    now,
		ExpiresAt:    now.Add(m.sessionTTL),
		LastUsed:     now,
		page:         page,
		cleanup:      cleanup,
		profileStore: m.profileStore,
	}

	m.mu.Lock()
	m.sessions[sess.ID] = sess
	m.mu.Unlock()

	log.Info().Str("session_id", sess.ID).Str("url", targetURL).Msg("session created")
	return sess, nil
}

// Get returns a session by ID, or an error if expired / not found.
func (m *Manager) Get(id string) (*BrowserSession, error) {
	m.mu.RLock()
	sess, ok := m.sessions[id]
	m.mu.RUnlock()

	if !ok {
		return nil, fmt.Errorf("session not found: %s", id)
	}

	sess.mu.Lock()
	defer sess.mu.Unlock()

	if sess.closed {
		return nil, fmt.Errorf("session already closed: %s", id)
	}

	now := time.Now()
	if now.After(sess.ExpiresAt) {
		go m.Destroy(id)
		return nil, fmt.Errorf("session expired: %s", id)
	}
	if now.Sub(sess.LastUsed) > m.inactivityTTL {
		go m.Destroy(id)
		return nil, fmt.Errorf("session inactive: %s", id)
	}

	sess.LastUsed = now
	return sess, nil
}

// Destroy closes a session and releases its browser page.
func (m *Manager) Destroy(id string) {
	m.mu.Lock()
	sess, ok := m.sessions[id]
	if ok {
		delete(m.sessions, id)
	}
	m.mu.Unlock()

	if ok {
		sess.close()
		log.Info().Str("session_id", id).Msg("session destroyed")
	}
}

// List returns summaries of all active sessions.
func (m *Manager) List() []SessionInfo {
	m.mu.RLock()
	defer m.mu.RUnlock()

	infos := make([]SessionInfo, 0, len(m.sessions))
	for _, sess := range m.sessions {
		infos = append(infos, SessionInfo{
			ID:        sess.ID,
			URL:       sess.URL,
			Profile:   sess.Profile,
			OrgID:     sess.OrgID,
			UserID:    sess.UserID,
			CreatedAt: sess.CreatedAt,
			ExpiresAt: sess.ExpiresAt,
			LastUsed:  sess.LastUsed,
			StepCount: sess.StepCount,
		})
	}
	return infos
}

// ListByOrg returns sessions belonging to a specific org. Empty orgID returns all.
func (m *Manager) ListByOrg(orgID string) []SessionInfo {
	if orgID == "" {
		return m.List()
	}
	m.mu.RLock()
	defer m.mu.RUnlock()

	infos := make([]SessionInfo, 0)
	for _, sess := range m.sessions {
		if sess.OrgID == orgID {
			infos = append(infos, SessionInfo{
				ID:        sess.ID,
				URL:       sess.URL,
				Profile:   sess.Profile,
				OrgID:     sess.OrgID,
				UserID:    sess.UserID,
				CreatedAt: sess.CreatedAt,
				ExpiresAt: sess.ExpiresAt,
				LastUsed:  sess.LastUsed,
				StepCount: sess.StepCount,
			})
		}
	}
	return infos
}

// GetForOrg returns a session only if it belongs to the given org.
// Empty orgID skips the ownership check (backward compat).
func (m *Manager) GetForOrg(id, orgID string) (*BrowserSession, error) {
	sess, err := m.Get(id)
	if err != nil {
		return nil, err
	}
	if orgID != "" && sess.OrgID != orgID {
		return nil, fmt.Errorf("session not found: %s", id)
	}
	return sess, nil
}

// ActiveCount returns the number of active sessions.
func (m *Manager) ActiveCount() int {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return len(m.sessions)
}

// Close stops the reaper and destroys all sessions.
func (m *Manager) Close() error {
	close(m.stop)

	m.mu.Lock()
	ids := make([]string, 0, len(m.sessions))
	for id := range m.sessions {
		ids = append(ids, id)
	}
	m.mu.Unlock()

	for _, id := range ids {
		m.Destroy(id)
	}
	return nil
}

func (m *Manager) reaper(interval time.Duration) {
	ticker := time.NewTicker(interval)
	defer ticker.Stop()

	for {
		select {
		case <-m.stop:
			return
		case <-ticker.C:
			m.reapExpired()
		}
	}
}

func (m *Manager) reapExpired() {
	now := time.Now()
	var expired []string

	m.mu.RLock()
	for id, sess := range m.sessions {
		sess.mu.Lock()
		isExpired := now.After(sess.ExpiresAt)
		isInactive := now.Sub(sess.LastUsed) > m.inactivityTTL
		sess.mu.Unlock()

		if isExpired || isInactive {
			expired = append(expired, id)
		}
	}
	m.mu.RUnlock()

	for _, id := range expired {
		log.Debug().Str("session_id", id).Msg("reaping expired session")
		m.Destroy(id)
	}
}

func (s *BrowserSession) close() {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.closed {
		return
	}

	if s.profileStore != nil && strings.TrimSpace(s.Profile) != "" && s.page != nil {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		profile, err := captureProfile(ctx, s.page, s.Profile)
		cancel()
		if err != nil {
			log.Warn().Err(err).Str("profile", s.Profile).Msg("failed to persist browser profile")
		} else {
			saveCtx, saveCancel := context.WithTimeout(context.Background(), 5*time.Second)
			if err := s.profileStore.Save(saveCtx, profile); err != nil {
				log.Warn().Err(err).Str("profile", s.Profile).Msg("failed to save browser profile")
			}
			saveCancel()
		}
	}

	s.closed = true

	if s.page != nil {
		_ = s.page.Close()
		s.page = nil
	}
	if s.cleanup != nil {
		s.cleanup()
		s.cleanup = nil
	}
}

func (m *Manager) applyProfile(ctx context.Context, page *rod.Page, targetURL, profileName string) error {
	if m == nil || m.profileStore == nil || page == nil || strings.TrimSpace(profileName) == "" {
		return nil
	}

	profile, err := m.profileStore.Load(ctx, profileName)
	if err != nil {
		return err
	}

	if len(profile.Cookies) > 0 {
		params := make([]*proto.NetworkCookieParam, 0, len(profile.Cookies))
		for _, cookie := range profile.Cookies {
			param := &proto.NetworkCookieParam{
				Name:     cookie.Name,
				Value:    cookie.Value,
				URL:      targetURL,
				Domain:   cookie.Domain,
				Path:     cookie.Path,
				Secure:   cookie.Secure,
				HTTPOnly: cookie.HTTPOnly,
			}
			if strings.TrimSpace(cookie.SameSite) != "" {
				param.SameSite = proto.NetworkCookieSameSite(cookie.SameSite)
			}
			params = append(params, param)
		}
		setCookies := proto.NetworkSetCookies{Cookies: params}
		if err := setCookies.Call(page); err != nil {
			return fmt.Errorf("apply cookies: %w", err)
		}
	}

	reloadNeeded := len(profile.Cookies) > 0
	if len(profile.LocalStore) > 0 {
		if _, err := page.Eval(`(items) => {
			for (const [key, value] of Object.entries(items || {})) {
				localStorage.setItem(key, String(value));
			}
			return true;
		}`, profile.LocalStore); err != nil {
			return fmt.Errorf("apply localStorage: %w", err)
		}
		reloadNeeded = true
	}

	if reloadNeeded {
		if err := page.Navigate(targetURL); err != nil {
			return fmt.Errorf("reload after profile apply: %w", err)
		}
		if err := page.WaitLoad(); err != nil {
			return fmt.Errorf("wait reload after profile apply: %w", err)
		}
	}

	return nil
}

func captureProfile(ctx context.Context, page *rod.Page, profileName string) (*Profile, error) {
	if page == nil {
		return nil, fmt.Errorf("page is nil")
	}

	info, err := page.Info()
	if err != nil {
		return nil, fmt.Errorf("page info: %w", err)
	}

	cookies, err := page.Cookies([]string{info.URL})
	if err != nil {
		return nil, fmt.Errorf("page cookies: %w", err)
	}

	localStorage := make(map[string]string)
	storageValue, err := page.Eval(`() => {
		const out = {};
		for (let i = 0; i < localStorage.length; i += 1) {
			const key = localStorage.key(i);
			out[key] = localStorage.getItem(key);
		}
		return out;
	}`)
	if err == nil && storageValue != nil && !storageValue.Value.Nil() {
		raw, marshalErr := json.Marshal(storageValue.Value.Raw())
		if marshalErr == nil {
			_ = json.Unmarshal(raw, &localStorage)
		}
	}

	entries := make([]CookieEntry, 0, len(cookies))
	for _, cookie := range cookies {
		if cookie == nil {
			continue
		}
		entries = append(entries, CookieEntry{
			Name:     cookie.Name,
			Value:    cookie.Value,
			Domain:   cookie.Domain,
			Path:     cookie.Path,
			Secure:   cookie.Secure,
			HTTPOnly: cookie.HTTPOnly,
			SameSite: string(cookie.SameSite),
		})
	}

	profile := &Profile{
		Name:       profileName,
		Cookies:    entries,
		LocalStore: localStorage,
	}
	if len(profile.Cookies) == 0 && len(profile.LocalStore) == 0 {
		parsed, parseErr := url.Parse(info.URL)
		if parseErr == nil && parsed != nil && parsed.Host != "" {
			profile.LocalStore = map[string]string{
				"last_origin": parsed.Scheme + "://" + parsed.Host,
			}
		}
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}
	return profile, nil
}
