package websites

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"
)

func validSession(s *Service) GoogleSession {
	return GoogleSession{SessionID: "session-test", AuthorizationURL: "https://accounts.google.com/o/oauth2/v2/auth?state=synthetic", ExpiresAt: s.now().Add(10 * time.Minute).UTC().Format(time.RFC3339)}
}

func TestGoogleConnectGuardsUntrustedResultsAndMissingProviders(t *testing.T) {
	s := newTestService(t, nil)
	w, _, err := s.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.ConnectGoogle(t.Context(), "web-missing"); !errors.Is(err, ErrNotFound) {
		t.Fatal("missing website accepted")
	}
	if _, err := s.ConnectGoogle(t.Context(), w.ID); !errors.Is(err, ErrUnavailable) {
		t.Fatal("missing broker accepted")
	}
	for _, tc := range []struct {
		name   string
		change func(*GoogleSession)
		fail   bool
	}{
		{"broker failure", func(*GoogleSession) {}, true},
		{"expired", func(v *GoogleSession) { v.ExpiresAt = s.now().Format(time.RFC3339) }, false},
		{"unbounded expiry", func(v *GoogleSession) { v.ExpiresAt = s.now().Add(2 * time.Hour).Format(time.RFC3339) }, false},
		{"bad timestamp", func(v *GoogleSession) { v.ExpiresAt = "tomorrow" }, false},
		{"unsafe session", func(v *GoogleSession) { v.SessionID = "../session" }, false},
		{"destination", func(v *GoogleSession) { v.AuthorizationURL = "https://evil.example/o/oauth2/v2/auth" }, false},
		{"credentials", func(v *GoogleSession) { v.AuthorizationURL = "https://user@accounts.google.com/o/oauth2/v2/auth" }, false},
		{"fragment", func(v *GoogleSession) { v.AuthorizationURL += "#unsafe" }, false},
		{"wrong path", func(v *GoogleSession) { v.AuthorizationURL = "https://accounts.google.com/not-oauth" }, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			value := validSession(s)
			tc.change(&value)
			s.SetGoogleActions(GoogleActions{Begin: func(context.Context) (GoogleSession, error) {
				if tc.fail {
					return GoogleSession{}, errors.New("secret detail")
				}
				return value, nil
			}})
			if got, err := s.ConnectGoogle(t.Context(), w.ID); !errors.Is(err, ErrUnavailable) || got.AuthorizationURL != "" {
				t.Fatalf("unsafe connection escaped: %v", err)
			}
		})
	}
}

func TestGoogleStatusRequiresWebsiteBoundLiveSessionAndNormalizesStates(t *testing.T) {
	s := newTestService(t, nil)
	w, _, err := s.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	s.SetGoogleActions(GoogleActions{Begin: func(context.Context) (GoogleSession, error) { return validSession(s), nil }})
	session, err := s.ConnectGoogle(t.Context(), w.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.GoogleStatus(t.Context(), w.ID, "someone-elses-session"); !errors.Is(err, ErrNotFound) {
		t.Fatal("foreign session accepted")
	}
	if _, err := s.GoogleStatus(t.Context(), "missing", session.SessionID); !errors.Is(err, ErrNotFound) {
		t.Fatal("missing website accepted")
	}
	if _, err := s.GoogleStatus(t.Context(), w.ID, session.SessionID); !errors.Is(err, ErrUnavailable) {
		t.Fatal("missing status broker accepted")
	}
	for _, tc := range []struct{ provider, want string }{{"pending", "pending"}, {"completed", "completed"}, {"connected", "completed"}, {"failed", "failed"}, {"error", "failed"}, {"expired", "expired"}, {"unknown", ""}} {
		s.SetGoogleActions(GoogleActions{Status: func(context.Context, string) (string, error) { return tc.provider, nil }})
		got, err := s.GoogleStatus(t.Context(), w.ID, session.SessionID)
		if got != tc.want || (tc.want == "") != (err != nil) {
			t.Fatalf("state %s => %s,%v", tc.provider, got, err)
		}
	}
	s.SetGoogleActions(GoogleActions{Status: func(context.Context, string) (string, error) { return "", errors.New("upstream private detail") }})
	if _, err := s.GoogleStatus(t.Context(), w.ID, session.SessionID); !errors.Is(err, ErrUnavailable) || strings.Contains(err.Error(), "private") {
		t.Fatal("broker error leaked")
	}
	changed, err := s.Change(t.Context(), w.ID, "https://other.example/", w.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.GoogleStatus(t.Context(), changed.ID, session.SessionID); !errors.Is(err, ErrNotFound) {
		t.Fatal("session survived URL change")
	}
	s.sessions[changed.ID] = pendingSession{site: changed.Site, id: "expired", expires: s.now().Add(-time.Second)}
	if got, err := s.GoogleStatus(t.Context(), changed.ID, "expired"); err != nil || got != "expired" {
		t.Fatal("expiry not enforced")
	}
}

func TestGoogleSessionCapacityPrunesExpiredEntries(t *testing.T) {
	s := newTestService(t, nil)
	w, _, err := s.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	s.SetGoogleActions(GoogleActions{Begin: func(context.Context) (GoogleSession, error) { return validSession(s), nil }})
	for i := range maxWebsites {
		s.sessions[fmt.Sprint(i)] = pendingSession{expires: s.now().Add(time.Hour)}
	}
	if _, err := s.ConnectGoogle(t.Context(), w.ID); !errors.Is(err, ErrUnavailable) {
		t.Fatal("session capacity not enforced")
	}
	for key, value := range s.sessions {
		value.expires = s.now().Add(-time.Second)
		s.sessions[key] = value
	}
	if _, err := s.ConnectGoogle(t.Context(), w.ID); err != nil || len(s.sessions) != 1 {
		t.Fatalf("expired entries were not pruned: %v", err)
	}
}
