package siteintegrations

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"sort"
)

// ConnectionFingerprint reads Integration metadata only. OAuth access/refresh
// tokens never leave Integration, and token refresh timestamps are excluded.
func (b *Broker) ConnectionFingerprint(ctx context.Context) (string, error) {
	var envelope struct {
		Success bool `json:"success"`
		Data    struct {
			Connections []connection `json:"connections"`
		} `json:"data"`
	}
	if b.request(ctx, "GET", "/api/v1/connections?providerKey=google", nil, &envelope) != nil || !envelope.Success || len(envelope.Data.Connections) > 20 {
		return "", errUnavailable
	}
	for _, c := range envelope.Data.Connections {
		if c.ProviderKey != "google" || !validID(c.ID) || len(c.Scopes) > 64 || len(c.Capabilities) > 64 {
			return "", errUnavailable
		}
	}
	return connectionFingerprint(envelope.Data.Connections), nil
}

func connectionFingerprint(items []connection) string {
	next := make([]connection, 0, len(items))
	for _, c := range items {
		c.Scopes = append([]string{}, c.Scopes...)
		c.Capabilities = append([]string{}, c.Capabilities...)
		sort.Strings(c.Scopes)
		sort.Strings(c.Capabilities)
		next = append(next, c)
	}
	sort.Slice(next, func(i, j int) bool { return next[i].ID < next[j].ID })
	raw, _ := json.Marshal(next)
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}
