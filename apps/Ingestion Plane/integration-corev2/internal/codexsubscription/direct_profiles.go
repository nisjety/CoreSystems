package codexsubscription

import (
	"slices"
	"sync"
	"time"
)

// modelProfile shapes the optional request fields for a model the pinned
// Codex 0.153.4 catalog (models.json) describes. The model allow-list itself
// stays in Model Plane (CODEX_SUBSCRIPTION_MODELS); an unknown model is still
// sent, just without these optional fields.
type modelProfile struct {
	verbosity    string
	serviceTiers []string
	lite         bool
}

var directModelProfiles = map[string]modelProfile{
	"gpt-6-astra":   {verbosity: "low", serviceTiers: []string{"priority"}, lite: true},
	"gpt-5.6-sol":   {verbosity: "low", serviceTiers: []string{"priority", "ultrafast"}, lite: true},
	"gpt-5.6-terra": {verbosity: "low", serviceTiers: []string{"priority"}, lite: true},
	"gpt-5.6-luna":  {verbosity: "low", serviceTiers: []string{"priority"}, lite: true},
	"gpt-5.5":       {verbosity: "low", serviceTiers: []string{"priority"}, lite: false},
}

func lookupModelProfile(model string) (modelProfile, bool) {
	profile, ok := directModelProfiles[model]
	return profile, ok
}

func (p modelProfile) supportsTier(tier string) bool {
	return slices.Contains(p.serviceTiers, tier)
}

// noPriorityWindow is how long a model whose priority tier the upstream
// rejected is sent without one.
const noPriorityWindow = time.Hour

// tierMemory remembers, per connection and model, that the upstream rejected
// the priority tier, so the next calls do not pay for the same rejection.
// Entitlement belongs to one ChatGPT account, so one connection's rejection
// never drops priority for another connection. In memory only.
type tierMemory struct {
	mu    sync.Mutex
	until map[string]time.Time
	now   func() time.Time
}

func newTierMemory() *tierMemory {
	return &tierMemory{until: make(map[string]time.Time), now: time.Now}
}

func tierMemoryKey(connectionID, model string) string {
	return connectionID + "\x00" + model
}

func (m *tierMemory) rejected(connectionID, model string) bool {
	key := tierMemoryKey(connectionID, model)
	m.mu.Lock()
	defer m.mu.Unlock()
	until, ok := m.until[key]
	if !ok {
		return false
	}
	if !m.now().Before(until) {
		delete(m.until, key)
		return false
	}
	return true
}

func (m *tierMemory) reject(connectionID, model string) {
	now := m.now()
	m.mu.Lock()
	defer m.mu.Unlock()
	for key, until := range m.until {
		// Keep the map bounded by the connections that are still rejected.
		if !now.Before(until) {
			delete(m.until, key)
		}
	}
	m.until[tierMemoryKey(connectionID, model)] = now.Add(noPriorityWindow)
}
