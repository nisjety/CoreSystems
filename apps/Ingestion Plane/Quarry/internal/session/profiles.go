package session

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
	"time"
)

// Profile stores persistent browser state (cookies, localStorage) across sessions.
type Profile struct {
	Name       string            `json:"name"`
	OrgID      string            `json:"org_id,omitempty"`
	UserID     string            `json:"user_id,omitempty"`
	Cookies    []CookieEntry     `json:"cookies,omitempty"`
	LocalStore map[string]string `json:"localStorage,omitempty"`
	CreatedAt  time.Time         `json:"created_at"`
	UpdatedAt  time.Time         `json:"updated_at"`
}

// CookieEntry represents a browser cookie for persistence.
type CookieEntry struct {
	Name     string `json:"name"`
	Value    string `json:"value"`
	Domain   string `json:"domain"`
	Path     string `json:"path"`
	Secure   bool   `json:"secure"`
	HTTPOnly bool   `json:"httpOnly"`
	SameSite string `json:"sameSite,omitempty"`
}

// ProfileStore provides CRUD for named browser profiles.
type ProfileStore interface {
	Save(ctx context.Context, profile *Profile) error
	Load(ctx context.Context, name string) (*Profile, error)
	Delete(ctx context.Context, name string) error
	List(ctx context.Context) ([]string, error)
}

// ScopedProfileKey builds a namespaced profile key: "org:user:name".
// When orgID or userID are empty, the corresponding segment is omitted
// for backward compatibility with un-scoped profiles.
func ScopedProfileKey(orgID, userID, name string) string {
	if orgID == "" && userID == "" {
		return name
	}
	if userID == "" {
		return orgID + ":" + name
	}
	return orgID + ":" + userID + ":" + name
}

// InMemoryProfileStore stores profiles in memory (suitable for single-instance deployments).
type InMemoryProfileStore struct {
	mu       sync.RWMutex
	profiles map[string]*Profile
}

func NewInMemoryProfileStore() *InMemoryProfileStore {
	return &InMemoryProfileStore{
		profiles: make(map[string]*Profile),
	}
}

func (s *InMemoryProfileStore) Save(_ context.Context, profile *Profile) error {
	if profile == nil || profile.Name == "" {
		return fmt.Errorf("profile name is required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()

	now := time.Now().UTC()
	existing, exists := s.profiles[profile.Name]
	if exists {
		existing.Cookies = cloneCookies(profile.Cookies)
		existing.LocalStore = cloneStringMap(profile.LocalStore)
		existing.UpdatedAt = now
	} else {
		stored := &Profile{
			Name:       profile.Name,
			Cookies:    cloneCookies(profile.Cookies),
			LocalStore: cloneStringMap(profile.LocalStore),
			CreatedAt:  now,
			UpdatedAt:  now,
		}
		s.profiles[profile.Name] = stored
	}
	return nil
}

func (s *InMemoryProfileStore) Load(_ context.Context, name string) (*Profile, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	p, ok := s.profiles[name]
	if !ok {
		return nil, fmt.Errorf("profile %q not found", name)
	}
	return &Profile{
		Name:       p.Name,
		Cookies:    cloneCookies(p.Cookies),
		LocalStore: cloneStringMap(p.LocalStore),
		CreatedAt:  p.CreatedAt,
		UpdatedAt:  p.UpdatedAt,
	}, nil
}

func (s *InMemoryProfileStore) Delete(_ context.Context, name string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.profiles, name)
	return nil
}

func (s *InMemoryProfileStore) List(_ context.Context) ([]string, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	names := make([]string, 0, len(s.profiles))
	for name := range s.profiles {
		names = append(names, name)
	}
	return names, nil
}

// SerializeProfile marshals a profile to JSON for external storage (Redis, MinIO).
func SerializeProfile(p *Profile) ([]byte, error) {
	return json.Marshal(p)
}

// DeserializeProfile unmarshals a profile from JSON.
func DeserializeProfile(data []byte) (*Profile, error) {
	var p Profile
	if err := json.Unmarshal(data, &p); err != nil {
		return nil, err
	}
	return &p, nil
}

func cloneCookies(cookies []CookieEntry) []CookieEntry {
	if cookies == nil {
		return nil
	}
	out := make([]CookieEntry, len(cookies))
	copy(out, cookies)
	return out
}

func cloneStringMap(m map[string]string) map[string]string {
	if m == nil {
		return nil
	}
	out := make(map[string]string, len(m))
	for k, v := range m {
		out[k] = v
	}
	return out
}
