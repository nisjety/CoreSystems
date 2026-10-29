package websites

import (
	"context"
	"net/url"
	"time"
)

// ConnectGoogle starts only an interactive consent request. Provider permission
// grants happen at Google's own screen; this service never confirms that UI.
func (s *Service) ConnectGoogle(ctx context.Context, id string) (GoogleSession, error) {
	record, err := s.Resolve(id, "")
	if err != nil {
		return GoogleSession{}, err
	}
	if s.google.Begin == nil {
		return GoogleSession{}, ErrUnavailable
	}
	session, err := s.google.Begin(ctx)
	if err != nil {
		return GoogleSession{}, ErrUnavailable
	}
	expires, err := time.Parse(time.RFC3339Nano, session.ExpiresAt)
	u, urlErr := url.Parse(session.AuthorizationURL)
	if err != nil || urlErr != nil || !expires.After(s.now()) || expires.After(s.now().Add(time.Hour)) || !identityPattern.MatchString(session.SessionID) || u.Scheme != "https" || u.Host != "accounts.google.com" || u.User != nil || u.Fragment != "" || u.Path != "/o/oauth2/v2/auth" && u.Path != "/o/oauth2/auth" {
		return GoogleSession{}, ErrUnavailable
	}
	s.sessionMu.Lock()
	defer s.sessionMu.Unlock()
	for key, pending := range s.sessions {
		if !pending.expires.After(s.now()) {
			delete(s.sessions, key)
		}
	}
	if len(s.sessions) >= maxWebsites {
		return GoogleSession{}, ErrUnavailable
	}
	s.sessions[id] = pendingSession{site: record.Website.Site, id: session.SessionID, expires: expires}
	return session, nil
}
func (s *Service) GoogleStatus(ctx context.Context, id, sessionID string) (string, error) {
	record, err := s.Resolve(id, "")
	if err != nil {
		return "", err
	}
	s.sessionMu.Lock()
	pending, ok := s.sessions[id]
	s.sessionMu.Unlock()
	if !ok || pending.id != sessionID || pending.site != record.Website.Site {
		return "", ErrNotFound
	}
	if !pending.expires.After(s.now()) {
		return "expired", nil
	}
	if s.google.Status == nil {
		return "", ErrUnavailable
	}
	status, err := s.google.Status(ctx, sessionID)
	if err != nil {
		return "", ErrUnavailable
	}
	switch status {
	case "pending", "completed", "failed", "expired":
		return status, nil
	case "connected":
		return "completed", nil
	case "error":
		return "failed", nil
	}
	return "", ErrUnavailable
}
