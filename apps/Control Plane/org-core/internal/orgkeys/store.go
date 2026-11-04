package orgkeys

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"sort"
	"sync"
	"time"
)

var (
	// ErrOrgNotFound: the organisation does not exist (404 org_not_found).
	ErrOrgNotFound = errors.New("orgkeys: organization not found")
	// ErrNoKey: the named KEK version does not exist for this org (404 key_not_found).
	ErrNoKey = errors.New("orgkeys: key not found")
	// ErrDestroyed: the org's keys were crypto-shredded (410 key_destroyed).
	ErrDestroyed = errors.New("orgkeys: keys destroyed")
	// ErrInvalidContext: the purpose, context or wrapped key is malformed, or the
	// wrapped key does not open under this org, purpose and context
	// (422 invalid_context).
	ErrInvalidContext = errors.New("orgkeys: invalid context")
	// ErrUnavailable: a master key is missing or wrong (canary), or the store is
	// unreachable (503 keys_unavailable).
	ErrUnavailable = errors.New("orgkeys: keys unavailable")
)

// KEK states. A retired KEK still unwraps (rotation never strands a DEK); a
// destroyed one has no key material left.
const (
	StateActive    = "active"
	StateRetired   = "retired"
	StateDestroyed = "destroyed"
)

// KEKRecord is one stored KEK version. Wrapped is nil once destroyed.
type KEKRecord struct {
	OrgID       string
	Version     int
	MasterKeyID string
	Wrapped     []byte
	State       string
	CreatedAt   time.Time
	RetiredAt   *time.Time
	DestroyedAt *time.Time
}

// BuildKEK makes the wrapped key material for a new KEK version under the store's
// per-org lock, so the version is fixed before the KEK is sealed to it.
type BuildKEK func(version int) (masterKeyID string, wrapped []byte, err error)

// Audit events. Each is content free: org, KEK version, purpose, the calling
// principal and a reason code, never a key, a wrapped key or a context value.
const (
	AuditKEKCreated   = "org_key.kek_created"
	AuditKEKRotated   = "org_key.kek_rotated"
	AuditKeysDestroy  = "org_key.keys_destroyed"
	AuditUnwrapDenied = "org_key.unwrap_denied"
	// AuditSubject is the one outbox subject the migration pins.
	AuditSubject = "verevon.audit.v2.control.org-core.org_key_custody"
)

// AuditEvent is one audit intent written beside the change it records.
type AuditEvent struct {
	OrgID      string
	Event      string
	Principal  string
	Purpose    string
	Reason     string
	KEKVersion int
	Count      int
}

// PublishFunc delivers one outbox row; the outbox marks it published on nil.
type PublishFunc func(ctx context.Context, subject, eventID string, payload map[string]any) error

// Store is the persistence seam: PostgresStore in production, MemoryStore in the
// host tests. Every org-scoped method answers ErrOrgNotFound for an unknown org
// and ErrDestroyed once the org's keys are shredded.
type Store interface {
	// Canaries returns every stored master-key canary by master id.
	Canaries(ctx context.Context) (map[string][]byte, error)
	// PutCanary stores a canary unless one exists and returns the stored one.
	PutCanary(ctx context.Context, masterKeyID string, sealed []byte) ([]byte, error)
	// EnsureActiveKEK returns the active KEK, creating the next version through
	// build under the org's lock when none is active (created reports that; the
	// creation audit event commits with it).
	EnsureActiveKEK(ctx context.Context, orgID string, build BuildKEK, audit AuditEvent) (rec KEKRecord, created bool, err error)
	// RotateKEK retires the active KEK and makes the next version active.
	RotateKEK(ctx context.Context, orgID string, build BuildKEK, audit AuditEvent) (KEKRecord, error)
	// GetKEK returns one version and the active version (0 when none).
	GetKEK(ctx context.Context, orgID string, version int) (rec KEKRecord, activeVersion int, err error)
	// ListKEKs returns every version, oldest first, key material included.
	ListKEKs(ctx context.Context, orgID string) ([]KEKRecord, error)
	// DestroyKEKs shreds every version (wrapped key set to NULL) and leaves a
	// tombstone so the org cannot silently get a fresh KEK afterwards. It returns
	// how many versions held key material.
	DestroyKEKs(ctx context.Context, orgID string, audit AuditEvent) (int, error)
	// RecordAudit writes one audit intent on its own (denied unwraps).
	RecordAudit(ctx context.Context, audit AuditEvent) error
	// FlushAudit publishes up to limit pending audit intents.
	FlushAudit(ctx context.Context, publish PublishFunc, limit int) (int, error)
}

func newAuditEventID() string {
	var raw [16]byte
	if _, err := rand.Read(raw[:]); err != nil {
		// crypto/rand does not fail on supported platforms; a time-based id keeps
		// the intent rather than dropping it.
		return "orgkeys:org-core:" + time.Now().UTC().Format("20060102T150405.000000000")
	}
	return "orgkeys:org-core:" + hex.EncodeToString(raw[:])
}

// auditPayload builds the content-free audit body in the shape of the other
// org-core outboxes.
func auditPayload(eventID string, event AuditEvent, occurredAt time.Time) map[string]any {
	outcome := "ok"
	if event.Event == AuditUnwrapDenied {
		outcome = "denied"
	}
	details := map[string]any{"kek_version": event.KEKVersion}
	if event.Purpose != "" {
		details["purpose"] = event.Purpose
	}
	if event.Reason != "" {
		details["reason"] = event.Reason
	}
	if event.Event == AuditKeysDestroy {
		details["versions_destroyed"] = event.Count
	}
	return map[string]any{
		"event_id": eventID, "occurred_at": occurredAt.UTC().Format(time.RFC3339Nano),
		"org_id": event.OrgID, "user_id": "service:" + event.Principal, "actor_role": "service",
		"plane": "control", "producer": "org-core", "event": event.Event,
		"subject": "org_keys:" + event.OrgID, "resource_id": event.OrgID, "outcome": outcome,
		"details": details,
	}
}

// MemoryStore is an in-process Store for host tests and local experiments. It is
// never wired in production (cmd/server uses PostgresStore).
type MemoryStore struct {
	mu        sync.Mutex
	orgs      map[string]bool
	keks      map[string][]KEKRecord
	canaries  map[string][]byte
	outbox    []memoryAuditRow
	now       func() time.Time
	FailReads error // when set, every org-scoped call returns it (store outage)
}

type memoryAuditRow struct {
	eventID   string
	subject   string
	payload   map[string]any
	published bool
}

// NewMemoryStore returns an empty store with the given organisations.
func NewMemoryStore(orgIDs ...string) *MemoryStore {
	store := &MemoryStore{
		orgs:     map[string]bool{},
		keks:     map[string][]KEKRecord{},
		canaries: map[string][]byte{},
		now:      time.Now,
	}
	for _, id := range orgIDs {
		store.orgs[id] = true
	}
	return store
}

// AddOrg creates an organisation.
func (m *MemoryStore) AddOrg(orgID string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.orgs[orgID] = true
}

// DeleteOrg removes an organisation and, like the FK cascade, its KEK rows.
func (m *MemoryStore) DeleteOrg(orgID string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	delete(m.orgs, orgID)
	delete(m.keks, orgID)
}

// SetCanary overwrites a stored canary (tests: a wrong master key).
func (m *MemoryStore) SetCanary(masterKeyID string, sealed []byte) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.canaries[masterKeyID] = append([]byte(nil), sealed...)
}

// AuditEvents returns the event names of every outbox row, in order.
func (m *MemoryStore) AuditEvents() []string {
	m.mu.Lock()
	defer m.mu.Unlock()
	events := make([]string, 0, len(m.outbox))
	for _, row := range m.outbox {
		if event, ok := row.payload["event"].(string); ok {
			events = append(events, event)
		}
	}
	return events
}

// AuditPayloads returns a copy of every outbox payload, in order.
func (m *MemoryStore) AuditPayloads() []map[string]any {
	m.mu.Lock()
	defer m.mu.Unlock()
	payloads := make([]map[string]any, 0, len(m.outbox))
	for _, row := range m.outbox {
		payloads = append(payloads, row.payload)
	}
	return payloads
}

func (m *MemoryStore) Canaries(_ context.Context) (map[string][]byte, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.FailReads != nil {
		return nil, m.FailReads
	}
	out := make(map[string][]byte, len(m.canaries))
	for id, sealed := range m.canaries {
		out[id] = append([]byte(nil), sealed...)
	}
	return out, nil
}

func (m *MemoryStore) PutCanary(_ context.Context, masterKeyID string, sealed []byte) ([]byte, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if existing, ok := m.canaries[masterKeyID]; ok {
		return append([]byte(nil), existing...), nil
	}
	m.canaries[masterKeyID] = append([]byte(nil), sealed...)
	return append([]byte(nil), sealed...), nil
}

// checkOrgLocked applies the common preconditions. The caller holds m.mu.
func (m *MemoryStore) checkOrgLocked(orgID string) error {
	if m.FailReads != nil {
		return m.FailReads
	}
	if !m.orgs[orgID] {
		return ErrOrgNotFound
	}
	for _, rec := range m.keks[orgID] {
		if rec.State == StateDestroyed {
			return ErrDestroyed
		}
	}
	return nil
}

func (m *MemoryStore) enqueueLocked(event AuditEvent) {
	eventID := newAuditEventID()
	m.outbox = append(m.outbox, memoryAuditRow{
		eventID: eventID, subject: AuditSubject, payload: auditPayload(eventID, event, m.now()),
	})
}

func (m *MemoryStore) nextVersionLocked(orgID string) int {
	next := 1
	for _, rec := range m.keks[orgID] {
		if rec.Version >= next {
			next = rec.Version + 1
		}
	}
	return next
}

func cloneRecord(rec KEKRecord) KEKRecord {
	rec.Wrapped = append([]byte(nil), rec.Wrapped...)
	if rec.Wrapped != nil && len(rec.Wrapped) == 0 {
		rec.Wrapped = nil
	}
	return rec
}

func (m *MemoryStore) insertLocked(orgID string, build BuildKEK) (KEKRecord, error) {
	version := m.nextVersionLocked(orgID)
	masterID, wrapped, err := build(version)
	if err != nil {
		return KEKRecord{}, err
	}
	rec := KEKRecord{
		OrgID: orgID, Version: version, MasterKeyID: masterID,
		Wrapped: append([]byte(nil), wrapped...), State: StateActive, CreatedAt: m.now().UTC(),
	}
	m.keks[orgID] = append(m.keks[orgID], rec)
	return cloneRecord(rec), nil
}

func (m *MemoryStore) EnsureActiveKEK(_ context.Context, orgID string, build BuildKEK, audit AuditEvent) (KEKRecord, bool, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if err := m.checkOrgLocked(orgID); err != nil {
		return KEKRecord{}, false, err
	}
	for _, rec := range m.keks[orgID] {
		if rec.State == StateActive {
			return cloneRecord(rec), false, nil
		}
	}
	rec, err := m.insertLocked(orgID, build)
	if err != nil {
		return KEKRecord{}, false, err
	}
	audit.KEKVersion = rec.Version
	m.enqueueLocked(audit)
	return rec, true, nil
}

func (m *MemoryStore) RotateKEK(_ context.Context, orgID string, build BuildKEK, audit AuditEvent) (KEKRecord, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if err := m.checkOrgLocked(orgID); err != nil {
		return KEKRecord{}, err
	}
	now := m.now().UTC()
	rows := m.keks[orgID]
	var retiredIndex = -1
	for i := range rows {
		if rows[i].State == StateActive {
			retiredIndex = i
		}
	}
	rec, err := m.insertLocked(orgID, build)
	if err != nil {
		return KEKRecord{}, err
	}
	if retiredIndex >= 0 {
		m.keks[orgID][retiredIndex].State = StateRetired
		m.keks[orgID][retiredIndex].RetiredAt = &now
	}
	audit.KEKVersion = rec.Version
	m.enqueueLocked(audit)
	return rec, nil
}

func (m *MemoryStore) GetKEK(_ context.Context, orgID string, version int) (KEKRecord, int, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if err := m.checkOrgLocked(orgID); err != nil {
		return KEKRecord{}, 0, err
	}
	active := 0
	var found *KEKRecord
	for i := range m.keks[orgID] {
		rec := m.keks[orgID][i]
		if rec.State == StateActive {
			active = rec.Version
		}
		if rec.Version == version {
			copied := cloneRecord(rec)
			found = &copied
		}
	}
	if found == nil {
		return KEKRecord{}, active, ErrNoKey
	}
	return *found, active, nil
}

func (m *MemoryStore) ListKEKs(_ context.Context, orgID string) ([]KEKRecord, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.FailReads != nil {
		return nil, m.FailReads
	}
	if !m.orgs[orgID] {
		return nil, ErrOrgNotFound
	}
	out := make([]KEKRecord, 0, len(m.keks[orgID]))
	for _, rec := range m.keks[orgID] {
		out = append(out, cloneRecord(rec))
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Version < out[j].Version })
	return out, nil
}

func (m *MemoryStore) DestroyKEKs(_ context.Context, orgID string, audit AuditEvent) (int, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if err := m.checkOrgLocked(orgID); err != nil {
		return 0, err
	}
	now := m.now().UTC()
	rows := m.keks[orgID]
	destroyed := 0
	for i := range rows {
		if rows[i].Wrapped != nil {
			destroyed++
		}
		zero(rows[i].Wrapped)
		rows[i].Wrapped = nil
		rows[i].State = StateDestroyed
		rows[i].DestroyedAt = &now
	}
	if len(rows) == 0 {
		rows = append(rows, KEKRecord{OrgID: orgID, Version: 0, State: StateDestroyed, CreatedAt: now, DestroyedAt: &now})
	}
	m.keks[orgID] = rows
	audit.Count = destroyed
	m.enqueueLocked(audit)
	return destroyed, nil
}

func (m *MemoryStore) RecordAudit(_ context.Context, audit AuditEvent) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.enqueueLocked(audit)
	return nil
}

func (m *MemoryStore) FlushAudit(ctx context.Context, publish PublishFunc, limit int) (int, error) {
	m.mu.Lock()
	pending := make([]int, 0)
	for i := range m.outbox {
		if !m.outbox[i].published && len(pending) < limit {
			pending = append(pending, i)
		}
	}
	rows := make([]memoryAuditRow, len(pending))
	for i, index := range pending {
		rows[i] = m.outbox[index]
	}
	m.mu.Unlock()

	published := 0
	var errs error
	for i, row := range rows {
		if err := publish(ctx, row.subject, row.eventID, row.payload); err != nil {
			errs = errors.Join(errs, err)
			continue
		}
		m.mu.Lock()
		m.outbox[pending[i]].published = true
		m.mu.Unlock()
		published++
	}
	return published, errs
}
