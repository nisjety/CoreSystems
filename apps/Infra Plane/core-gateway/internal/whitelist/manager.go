package whitelist

import (
	"fmt"
	"net"
	"os"
	"sync"
	"time"

	"github.com/coresystem/integration-gateway/internal/audit"
	"gopkg.in/yaml.v3"
)

// Manager handles IP whitelist and blacklist management
type Manager struct {
	whitelist         []IPEntry
	blacklist         []IPEntry
	traefikConfigPath string
	audit             *audit.AuditLogger
	mu                sync.RWMutex
}

// IPEntry represents an IP address or CIDR range with metadata
type IPEntry struct {
	IP          string     `json:"ip" yaml:"ip"`
	Description string     `json:"description" yaml:"description"`
	AddedAt     time.Time  `json:"added_at" yaml:"added_at"`
	AddedBy     string     `json:"added_by" yaml:"added_by"`
	ExpiresAt   *time.Time `json:"expires_at,omitempty" yaml:"expires_at,omitempty"`
}

// Config holds whitelist manager configuration
type Config struct {
	TraefikConfigPath string
	AuditLogger       *audit.AuditLogger
}

// TraefikDynamicConfig represents Traefik dynamic configuration
type TraefikDynamicConfig struct {
	HTTP struct {
		Middlewares map[string]struct {
			IPWhiteList struct {
				SourceRange []string `yaml:"sourceRange"`
			} `yaml:"ipWhiteList"`
		} `yaml:"middlewares"`
	} `yaml:"http"`
}

// NewManager creates a new whitelist manager
func NewManager(cfg Config) (*Manager, error) {
	m := &Manager{
		whitelist:         make([]IPEntry, 0),
		blacklist:         make([]IPEntry, 0),
		traefikConfigPath: cfg.TraefikConfigPath,
		audit:             cfg.AuditLogger,
	}

	// Load existing whitelist from Traefik config
	if err := m.loadFromTraefikConfig(); err != nil {
		return nil, fmt.Errorf("failed to load whitelist: %w", err)
	}

	return m, nil
}

// AddToWhitelist adds an IP to the whitelist
func (m *Manager) AddToWhitelist(ip, description, addedBy string, expiresAt *time.Time) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	// Validate IP/CIDR
	if err := validateIPOrCIDR(ip); err != nil {
		return fmt.Errorf("invalid IP or CIDR: %w", err)
	}

	// Check if already exists
	for _, entry := range m.whitelist {
		if entry.IP == ip {
			return fmt.Errorf("IP %s already in whitelist", ip)
		}
	}

	// Add to whitelist
	entry := IPEntry{
		IP:          ip,
		Description: description,
		AddedAt:     time.Now(),
		AddedBy:     addedBy,
		ExpiresAt:   expiresAt,
	}
	m.whitelist = append(m.whitelist, entry)

	// Update Traefik config
	if err := m.updateTraefikConfig(); err != nil {
		// Rollback
		m.whitelist = m.whitelist[:len(m.whitelist)-1]
		return fmt.Errorf("failed to update Traefik config: %w", err)
	}

	// Log the action
	if m.audit != nil {
		m.audit.LogEvent(audit.AuditEvent{
			Timestamp: time.Now(),
			Action:    "whitelist_add",
			Actor:     addedBy,
			Resource:  ip,
			Success:   true,
			Details: map[string]string{
				"description": description,
			},
		})
	}

	return nil
}

// RemoveFromWhitelist removes an IP from the whitelist
func (m *Manager) RemoveFromWhitelist(ip, removedBy string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	// Find and remove
	found := false
	newWhitelist := make([]IPEntry, 0, len(m.whitelist))
	for _, entry := range m.whitelist {
		if entry.IP != ip {
			newWhitelist = append(newWhitelist, entry)
		} else {
			found = true
		}
	}

	if !found {
		return fmt.Errorf("IP %s not found in whitelist", ip)
	}

	m.whitelist = newWhitelist

	// Update Traefik config
	if err := m.updateTraefikConfig(); err != nil {
		return fmt.Errorf("failed to update Traefik config: %w", err)
	}

	// Log the action
	if m.audit != nil {
		m.audit.LogEvent(audit.AuditEvent{
			Timestamp: time.Now(),
			Action:    "whitelist_remove",
			Actor:     removedBy,
			Resource:  ip,
			Success:   true,
		})
	}

	return nil
}

// GetWhitelist returns the current whitelist
func (m *Manager) GetWhitelist() []IPEntry {
	m.mu.RLock()
	defer m.mu.RUnlock()

	// Return a copy
	result := make([]IPEntry, len(m.whitelist))
	copy(result, m.whitelist)
	return result
}

// AddToBlacklist adds an IP to the blacklist
func (m *Manager) AddToBlacklist(ip, description, addedBy string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	// Validate IP/CIDR
	if err := validateIPOrCIDR(ip); err != nil {
		return fmt.Errorf("invalid IP or CIDR: %w", err)
	}

	// Check if already exists
	for _, entry := range m.blacklist {
		if entry.IP == ip {
			return fmt.Errorf("IP %s already in blacklist", ip)
		}
	}

	// Add to blacklist
	entry := IPEntry{
		IP:          ip,
		Description: description,
		AddedAt:     time.Now(),
		AddedBy:     addedBy,
	}
	m.blacklist = append(m.blacklist, entry)

	// Log the action
	if m.audit != nil {
		m.audit.LogEvent(audit.AuditEvent{
			Timestamp: time.Now(),
			Action:    "blacklist_add",
			Actor:     addedBy,
			Resource:  ip,
			Success:   true,
			Details: map[string]string{
				"description": description,
			},
		})
	}

	return nil
}

// RemoveFromBlacklist removes an IP from the blacklist
func (m *Manager) RemoveFromBlacklist(ip, removedBy string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	// Find and remove
	found := false
	newBlacklist := make([]IPEntry, 0, len(m.blacklist))
	for _, entry := range m.blacklist {
		if entry.IP != ip {
			newBlacklist = append(newBlacklist, entry)
		} else {
			found = true
		}
	}

	if !found {
		return fmt.Errorf("IP %s not found in blacklist", ip)
	}

	m.blacklist = newBlacklist

	// Log the action
	if m.audit != nil {
		m.audit.LogEvent(audit.AuditEvent{
			Timestamp: time.Now(),
			Action:    "blacklist_remove",
			Actor:     removedBy,
			Resource:  ip,
			Success:   true,
		})
	}

	return nil
}

// GetBlacklist returns the current blacklist
func (m *Manager) GetBlacklist() []IPEntry {
	m.mu.RLock()
	defer m.mu.RUnlock()

	// Return a copy
	result := make([]IPEntry, len(m.blacklist))
	copy(result, m.blacklist)
	return result
}

// IsAllowed checks if an IP is allowed (in whitelist and not in blacklist)
func (m *Manager) IsAllowed(ip string) bool {
	m.mu.RLock()
	defer m.mu.RUnlock()

	// Parse IP
	parsedIP := net.ParseIP(ip)
	if parsedIP == nil {
		return false
	}

	// Check blacklist first
	for _, entry := range m.blacklist {
		if m.ipMatches(parsedIP, entry.IP) {
			return false
		}
	}

	// Check whitelist
	for _, entry := range m.whitelist {
		// Check expiration
		if entry.ExpiresAt != nil && time.Now().After(*entry.ExpiresAt) {
			continue
		}

		if m.ipMatches(parsedIP, entry.IP) {
			return true
		}
	}

	return false
}

// ipMatches checks if an IP matches an IP or CIDR range
func (m *Manager) ipMatches(ip net.IP, pattern string) bool {
	// Try as CIDR first
	_, ipNet, err := net.ParseCIDR(pattern)
	if err == nil {
		return ipNet.Contains(ip)
	}

	// Try as IP
	patternIP := net.ParseIP(pattern)
	if patternIP != nil {
		return ip.Equal(patternIP)
	}

	return false
}

// loadFromTraefikConfig loads the whitelist from Traefik dynamic config
func (m *Manager) loadFromTraefikConfig() error {
	// Read Traefik config file
	data, err := os.ReadFile(m.traefikConfigPath)
	if err != nil {
		if os.IsNotExist(err) {
			// File doesn't exist, start with empty whitelist
			return nil
		}
		return fmt.Errorf("failed to read Traefik config: %w", err)
	}

	var config TraefikDynamicConfig
	if err := yaml.Unmarshal(data, &config); err != nil {
		return fmt.Errorf("failed to parse Traefik config: %w", err)
	}

	// Extract IPs from dynamic-ipwhitelist middleware
	if middleware, ok := config.HTTP.Middlewares["dynamic-ipwhitelist"]; ok {
		for _, ip := range middleware.IPWhiteList.SourceRange {
			m.whitelist = append(m.whitelist, IPEntry{
				IP:          ip,
				Description: "Existing from Traefik config",
				AddedAt:     time.Now(),
				AddedBy:     "system",
			})
		}
	}

	return nil
}

// updateTraefikConfig updates the Traefik dynamic configuration file
func (m *Manager) updateTraefikConfig() error {
	// Build source range list
	sourceRange := make([]string, 0, len(m.whitelist))
	for _, entry := range m.whitelist {
		// Skip expired entries
		if entry.ExpiresAt != nil && time.Now().After(*entry.ExpiresAt) {
			continue
		}
		sourceRange = append(sourceRange, entry.IP)
	}

	// Create Traefik config structure
	config := TraefikDynamicConfig{}
	config.HTTP.Middlewares = make(map[string]struct {
		IPWhiteList struct {
			SourceRange []string `yaml:"sourceRange"`
		} `yaml:"ipWhiteList"`
	})

	middleware := config.HTTP.Middlewares["dynamic-ipwhitelist"]
	middleware.IPWhiteList.SourceRange = sourceRange
	config.HTTP.Middlewares["dynamic-ipwhitelist"] = middleware

	// Marshal to YAML
	yamlData, err := yaml.Marshal(&config)
	if err != nil {
		return fmt.Errorf("failed to marshal config: %w", err)
	}

	// Add header comment
	header := "# Traefik Dynamic Configuration - IP Whitelist\n" +
		"# Managed by Integration Gateway\n" +
		fmt.Sprintf("# Last updated: %s\n\n", time.Now().Format("2006-01-02 15:04:05"))

	// Write to file
	if err := os.WriteFile(m.traefikConfigPath, []byte(header+string(yamlData)), 0644); err != nil {
		return fmt.Errorf("failed to write config file: %w", err)
	}

	return nil
}

// CleanupExpired removes expired whitelist entries
func (m *Manager) CleanupExpired() int {
	m.mu.Lock()
	defer m.mu.Unlock()

	now := time.Now()
	removed := 0
	newWhitelist := make([]IPEntry, 0, len(m.whitelist))

	for _, entry := range m.whitelist {
		if entry.ExpiresAt != nil && now.After(*entry.ExpiresAt) {
			removed++
			if m.audit != nil {
				m.audit.LogEvent(audit.AuditEvent{
					Timestamp: now,
					Action:    "whitelist_expired",
					Actor:     "system",
					Resource:  entry.IP,
					Success:   true,
				})
			}
		} else {
			newWhitelist = append(newWhitelist, entry)
		}
	}

	if removed > 0 {
		m.whitelist = newWhitelist
		_ = m.updateTraefikConfig()
	}

	return removed
}

// validateIPOrCIDR validates an IP address or CIDR range
func validateIPOrCIDR(ipStr string) error {
	// Try parsing as CIDR
	_, _, err := net.ParseCIDR(ipStr)
	if err == nil {
		return nil
	}

	// Try parsing as IP
	ip := net.ParseIP(ipStr)
	if ip == nil {
		return fmt.Errorf("invalid IP address or CIDR range")
	}

	return nil
}
