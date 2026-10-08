package store

import (
	"context"
	"sort"
	"strings"
	"time"

	"github.com/google/uuid"
)

// The in-memory implementation of ConnectionZDRStore mirrors the PostgreSQL one statement for
// statement (postgres_zdr.go): the same checks in the same order, the same errors. The shared
// contract suite (storetest) runs against both.

func (r *MemoryRepository) ApplyConnectionZDRAssessment(_ context.Context, assessment ConnectionZDRAssessment) (ConnectionZDRApplyResult, error) {
	if err := assessment.Validate(); err != nil {
		return ConnectionZDRApplyResult{}, err
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.connections[assessment.ConnectionID]; !ok {
		return ConnectionZDRApplyResult{}, ErrNotFound
	}
	next := assessment.record(time.Now().UTC())
	var result ConnectionZDRApplyResult
	existing, found := r.zdrRecords[assessment.ConnectionID]
	switch {
	case !found:
		r.zdrRecords[assessment.ConnectionID] = next
		result.Record = next
		result.Created = true
	case existing.Digest != next.Digest || existing.BasisRef != next.BasisRef:
		previous := existing
		next.CreatedAt = existing.CreatedAt
		r.zdrRecords[assessment.ConnectionID] = next
		result.Record = next
		result.Previous = &previous
		result.Changed = existing.Digest != next.Digest
	default:
		previous := existing
		result.Record = existing
		result.Previous = &previous
	}
	// Every active acknowledgement bound to another digest is stale. Revoking them here, in
	// the same unit of work as the record, is what makes a capability change take effect; doing
	// it on every apply (not only when the digest changed) heals a missed revocation.
	revokedAt := next.CheckedAt
	for index, ack := range r.zdrAcks {
		if ack.ConnectionID != assessment.ConnectionID || ack.RevokedAt != nil || ack.Digest == result.Record.Digest {
			continue
		}
		stamp := revokedAt
		ack.RevokedAt = &stamp
		ack.RevokeReason = ZDRRevokeCapabilityChanged
		r.zdrAcks[index] = ack
		result.Revoked = append(result.Revoked, cloneZDRAcknowledgement(ack))
	}
	return result, nil
}

func (r *MemoryRepository) GetConnectionZDR(_ context.Context, connectionID string) (ConnectionZDR, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	record, ok := r.zdrRecords[connectionID]
	if !ok {
		return ConnectionZDR{}, ErrNotFound
	}
	return record, nil
}

func (r *MemoryRepository) InsertConnectionZDRAcknowledgement(_ context.Context, in NewConnectionZDRAcknowledgement) (ConnectionZDRAcknowledgement, bool, error) {
	if err := in.Validate(); err != nil {
		return ConnectionZDRAcknowledgement{}, false, err
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	connection, ok := r.connections[in.ConnectionID]
	if !ok || connection.DeletedAt != nil || connection.OrganizationID != in.OrganizationID || connection.UserID != in.UserID {
		return ConnectionZDRAcknowledgement{}, false, ErrNotFound
	}
	record, ok := r.zdrRecords[in.ConnectionID]
	if !ok {
		return ConnectionZDRAcknowledgement{}, false, ErrNotFound
	}
	if !record.Capability.RequiresAcknowledgement() {
		return ConnectionZDRAcknowledgement{}, false, invalidZDR("a %s connection needs no acknowledgement", record.Capability)
	}
	if record.Digest != in.Digest {
		return ConnectionZDRAcknowledgement{}, false, ErrConflict
	}
	for _, existing := range r.zdrAcks {
		if existing.ConnectionID == in.ConnectionID && existing.Digest == in.Digest && existing.RevokedAt == nil {
			return cloneZDRAcknowledgement(existing), false, nil
		}
	}
	id := strings.TrimSpace(in.ID)
	if id == "" {
		id = "zdrack_" + uuid.NewString()
	}
	for _, existing := range r.zdrAcks {
		if existing.ID == id {
			return ConnectionZDRAcknowledgement{}, false, ErrConflict
		}
	}
	at := in.AcknowledgedAt
	if at.IsZero() {
		at = time.Now().UTC()
	}
	saved := ConnectionZDRAcknowledgement{
		ID:              id,
		OrganizationID:  connection.OrganizationID,
		ConnectionID:    connection.ID,
		UserID:          connection.UserID,
		ProviderKey:     record.ProviderKey,
		Capability:      record.Capability,
		Basis:           record.Basis,
		CatalogVersion:  record.CatalogVersion,
		Digest:          record.Digest,
		TextVersion:     in.TextVersion,
		Locale:          in.Locale,
		TextSHA256:      in.TextSHA256,
		TextSnapshot:    in.TextSnapshot,
		OrgZDRAtAck:     in.OrgZDRAtAck,
		AcknowledgedVia: in.AcknowledgedVia,
		RequestID:       in.RequestID,
		AcknowledgedAt:  at.UTC(),
	}
	r.zdrAcks = append(r.zdrAcks, saved)
	return cloneZDRAcknowledgement(saved), true, nil
}

func (r *MemoryRepository) ListConnectionZDRAcknowledgements(_ context.Context, connectionID string, activeOnly bool) ([]ConnectionZDRAcknowledgement, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := []ConnectionZDRAcknowledgement{}
	for _, ack := range r.zdrAcks {
		if ack.ConnectionID != connectionID || (activeOnly && ack.RevokedAt != nil) {
			continue
		}
		out = append(out, cloneZDRAcknowledgement(ack))
	}
	sort.SliceStable(out, func(i, j int) bool {
		if !out[i].AcknowledgedAt.Equal(out[j].AcknowledgedAt) {
			return out[i].AcknowledgedAt.After(out[j].AcknowledgedAt)
		}
		return out[i].ID < out[j].ID
	})
	return out, nil
}

func (r *MemoryRepository) RevokeConnectionZDRAcknowledgements(_ context.Context, connectionID, reason string, at time.Time) ([]ConnectionZDRAcknowledgement, error) {
	if !ValidZDRRevokeReason(reason) {
		return nil, invalidZDR("revoke reason %q is not one of %s", reason, strings.Join(zdrRevokeReasons, ", "))
	}
	if at.IsZero() {
		at = time.Now().UTC()
	}
	at = at.UTC()
	r.mu.Lock()
	defer r.mu.Unlock()
	revoked := []ConnectionZDRAcknowledgement{}
	for index, ack := range r.zdrAcks {
		if ack.ConnectionID != connectionID || ack.RevokedAt != nil {
			continue
		}
		stamp := at
		ack.RevokedAt = &stamp
		ack.RevokeReason = reason
		r.zdrAcks[index] = ack
		revoked = append(revoked, cloneZDRAcknowledgement(ack))
	}
	return revoked, nil
}
