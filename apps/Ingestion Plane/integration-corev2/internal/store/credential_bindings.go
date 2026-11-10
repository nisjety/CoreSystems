package store

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// Credential bindings (W4-CC-3, ADR-NEXT-sandbox-credential-injection sections 1 and 4).
//
// A binding ties one sealed secret to one host, one header, and the methods and path prefixes that
// may be forwarded with it. The secret arrives here already sealed (the API seals it with the
// credential vault, additional data = the binding id); this store never sees plaintext. Metadata
// reads never select the ciphertext, and a revoke empties it.
//
// Both implementations (PostgreSQL, migration 0015_credential_bindings.sql, and in-memory) are
// scoped by organisation in every statement: a binding id from another organisation is simply not
// found.

// Credential binding kinds. They describe what the secret is, not where it is used.
const (
	// CredentialBindingKindMCPBearer is a bearer token for an MCP server.
	CredentialBindingKindMCPBearer = "mcp_bearer"
	// CredentialBindingKindRESTAPIKey is an API key sent in a named header of a REST API.
	CredentialBindingKindRESTAPIKey = "rest_api_key"
	// CredentialBindingKindRESTBearer is a bearer token for a REST API.
	CredentialBindingKindRESTBearer = "rest_bearer"
)

// Credential binding statuses.
const (
	// CredentialBindingStatusActive is a binding whose secret is held and may be redeemed.
	CredentialBindingStatusActive = "active"
	// CredentialBindingStatusRevoked is a binding that was revoked; its ciphertext is gone.
	CredentialBindingStatusRevoked = "revoked"
)

// CredentialBinding is one stored binding. SecretCiphertext holds the sealed secret and is never
// serialised; it is set on create and is empty on every read this store offers.
type CredentialBinding struct {
	ID                  string     `json:"id"`
	OrganizationID      string     `json:"organizationId"`
	CreatedByUserID     string     `json:"createdByUserId"`
	Kind                string     `json:"kind"`
	Host                string     `json:"host"`
	HeaderName          string     `json:"headerName"`
	AllowedMethods      []string   `json:"allowedMethods"`
	AllowedPathPrefixes []string   `json:"allowedPathPrefixes"`
	KeyVersion          string     `json:"keyVersion"`
	Status              string     `json:"status"`
	CreatedAt           time.Time  `json:"createdAt"`
	RevokedAt           *time.Time `json:"revokedAt,omitempty"`
	// SecretCiphertext is the vault-sealed secret. It is write-only through this store.
	SecretCiphertext []byte `json:"-"`
}

// String and GoString omit the ciphertext so a stray %v or %#v cannot print it.
func (b CredentialBinding) String() string   { return "store.CredentialBinding{" + b.ID + "}" }
func (b CredentialBinding) GoString() string { return b.String() }

// CredentialBindingStore keeps credential bindings. It is part of AuditTransaction so a create or
// revoke commits with the audit row that records it.
type CredentialBindingStore interface {
	// CreateCredentialBinding inserts an active binding. The id, organisation, creator, kind, host,
	// header and ciphertext are required; an id that already exists is ErrConflict.
	CreateCredentialBinding(ctx context.Context, binding CredentialBinding) (CredentialBinding, error)
	// ListCredentialBindings returns an organisation's bindings, newest first, as metadata only
	// (never the ciphertext).
	ListCredentialBindings(ctx context.Context, organizationID string) ([]CredentialBinding, error)
	// RevokeCredentialBinding revokes an organisation's active binding and empties its ciphertext.
	// A binding that does not exist, belongs to another organisation or is already revoked is
	// ErrNotFound.
	RevokeCredentialBinding(ctx context.Context, organizationID, id string, revokedAt time.Time) (CredentialBinding, error)
	// PurgeOrganizationCredentialBindings erases every binding of an organisation (the organisation
	// purge) and returns how many rows it removed.
	PurgeOrganizationCredentialBindings(ctx context.Context, organizationID string) (int, error)
	// GetCredentialBinding returns an organisation's binding by id as metadata only, whatever its
	// status (W4-CC-4 seal route). A binding of another organisation or an unknown id is ErrNotFound.
	GetCredentialBinding(ctx context.Context, organizationID, id string) (CredentialBinding, error)
	// GetCredentialBindingForRedeem returns an organisation's binding with its sealed ciphertext, in
	// one read, for the redeem route only (W4-CC-4). A revoked binding comes back with an empty
	// ciphertext, so the caller must check the status first.
	GetCredentialBindingForRedeem(ctx context.Context, organizationID, id string) (CredentialBinding, error)
}

func (b CredentialBinding) validateForCreate() error {
	switch {
	case strings.TrimSpace(b.ID) == "":
		return errors.New("credential binding id is required")
	case strings.TrimSpace(b.OrganizationID) == "":
		return errors.New("credential binding organization is required")
	case strings.TrimSpace(b.CreatedByUserID) == "":
		return errors.New("credential binding creator is required")
	case strings.TrimSpace(b.Kind) == "", strings.TrimSpace(b.Host) == "", strings.TrimSpace(b.HeaderName) == "":
		return errors.New("credential binding kind, host and header are required")
	case len(b.SecretCiphertext) == 0:
		return errors.New("credential binding ciphertext is required")
	case strings.TrimSpace(b.KeyVersion) == "":
		return errors.New("credential binding key version is required")
	}
	return nil
}

func cloneCredentialBinding(b CredentialBinding) CredentialBinding {
	b.AllowedMethods = nonNilStringSlice(b.AllowedMethods)
	b.AllowedPathPrefixes = nonNilStringSlice(b.AllowedPathPrefixes)
	b.SecretCiphertext = append([]byte(nil), b.SecretCiphertext...)
	if b.RevokedAt != nil {
		revokedAt := *b.RevokedAt
		b.RevokedAt = &revokedAt
	}
	return b
}

// metadata returns the binding without its ciphertext.
func (b CredentialBinding) metadata() CredentialBinding {
	b = cloneCredentialBinding(b)
	b.SecretCiphertext = nil
	return b
}

func cloneCredentialBindings(input map[string]CredentialBinding) map[string]CredentialBinding {
	output := make(map[string]CredentialBinding, len(input))
	for key, value := range input {
		output[key] = cloneCredentialBinding(value)
	}
	return output
}

// In-memory implementation.

func (r *MemoryRepository) CreateCredentialBinding(_ context.Context, binding CredentialBinding) (CredentialBinding, error) {
	if err := binding.validateForCreate(); err != nil {
		return CredentialBinding{}, err
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, exists := r.credentialBindings[binding.ID]; exists {
		return CredentialBinding{}, ErrConflict
	}
	if binding.CreatedAt.IsZero() {
		binding.CreatedAt = time.Now().UTC()
	}
	binding.Status = CredentialBindingStatusActive
	binding.RevokedAt = nil
	stored := cloneCredentialBinding(binding)
	r.credentialBindings[stored.ID] = stored
	return stored.metadata(), nil
}

func (r *MemoryRepository) ListCredentialBindings(_ context.Context, organizationID string) ([]CredentialBinding, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := []CredentialBinding{}
	for _, stored := range r.credentialBindings {
		if stored.OrganizationID == organizationID {
			out = append(out, stored.metadata())
		}
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].CreatedAt.Equal(out[j].CreatedAt) {
			return out[i].ID > out[j].ID
		}
		return out[i].CreatedAt.After(out[j].CreatedAt)
	})
	return out, nil
}

func (r *MemoryRepository) RevokeCredentialBinding(_ context.Context, organizationID, id string, revokedAt time.Time) (CredentialBinding, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	stored, ok := r.credentialBindings[id]
	if !ok || stored.OrganizationID != organizationID || stored.Status != CredentialBindingStatusActive {
		return CredentialBinding{}, ErrNotFound
	}
	stamp := revokedAt.UTC()
	stored.Status = CredentialBindingStatusRevoked
	stored.RevokedAt = &stamp
	stored.SecretCiphertext = nil
	r.credentialBindings[id] = stored
	return stored.metadata(), nil
}

func (r *MemoryRepository) PurgeOrganizationCredentialBindings(_ context.Context, organizationID string) (int, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	purged := 0
	for id, stored := range r.credentialBindings {
		if stored.OrganizationID == organizationID {
			delete(r.credentialBindings, id)
			purged++
		}
	}
	return purged, nil
}

// PostgreSQL implementation (migration 0015_credential_bindings.sql).

// credentialBindingMetadataColumns is every column except the ciphertext: no list or revoke
// statement ever selects the sealed secret.
const credentialBindingMetadataColumns = "id, organization_id, created_by_user_id, kind, host, header_name, allowed_methods, allowed_path_prefixes, key_version, status, created_at, revoked_at"

func scanCredentialBinding(row pgx.Row) (CredentialBinding, error) {
	var binding CredentialBinding
	if err := row.Scan(
		&binding.ID, &binding.OrganizationID, &binding.CreatedByUserID, &binding.Kind, &binding.Host, &binding.HeaderName,
		&binding.AllowedMethods, &binding.AllowedPathPrefixes, &binding.KeyVersion, &binding.Status, &binding.CreatedAt, &binding.RevokedAt,
	); err != nil {
		return CredentialBinding{}, err
	}
	binding.AllowedMethods = nonNilStringSlice(binding.AllowedMethods)
	binding.AllowedPathPrefixes = nonNilStringSlice(binding.AllowedPathPrefixes)
	return binding, nil
}

func (r *PostgresRepository) CreateCredentialBinding(ctx context.Context, binding CredentialBinding) (CredentialBinding, error) {
	if err := binding.validateForCreate(); err != nil {
		return CredentialBinding{}, err
	}
	if binding.CreatedAt.IsZero() {
		binding.CreatedAt = time.Now().UTC()
	}
	created, err := scanCredentialBinding(r.pool.QueryRow(ctx, `
		INSERT INTO credential_bindings (
			id, organization_id, created_by_user_id, kind, host, header_name,
			allowed_methods, allowed_path_prefixes, secret_ciphertext, key_version, status, created_at
		) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'active',$11)
		RETURNING `+credentialBindingMetadataColumns,
		binding.ID, binding.OrganizationID, binding.CreatedByUserID, binding.Kind, binding.Host, binding.HeaderName,
		nonNilStringSlice(binding.AllowedMethods), nonNilStringSlice(binding.AllowedPathPrefixes),
		binding.SecretCiphertext, binding.KeyVersion, binding.CreatedAt))
	if err != nil {
		if pgErrorCode(err) == pgUniqueViolation {
			return CredentialBinding{}, ErrConflict
		}
		return CredentialBinding{}, fmt.Errorf("create credential binding: %w", err)
	}
	return created, nil
}

func (r *PostgresRepository) ListCredentialBindings(ctx context.Context, organizationID string) ([]CredentialBinding, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT `+credentialBindingMetadataColumns+`
		FROM credential_bindings
		WHERE organization_id = $1
		ORDER BY created_at DESC, id DESC
	`, organizationID)
	if err != nil {
		return nil, fmt.Errorf("list credential bindings: %w", err)
	}
	defer rows.Close()
	out := []CredentialBinding{}
	for rows.Next() {
		binding, err := scanCredentialBinding(rows)
		if err != nil {
			return nil, fmt.Errorf("scan credential binding: %w", err)
		}
		out = append(out, binding)
	}
	return out, rows.Err()
}

func (r *PostgresRepository) RevokeCredentialBinding(ctx context.Context, organizationID, id string, revokedAt time.Time) (CredentialBinding, error) {
	revoked, err := scanCredentialBinding(r.pool.QueryRow(ctx, `
		UPDATE credential_bindings
		SET status = 'revoked', revoked_at = $3, secret_ciphertext = '\x'::bytea
		WHERE organization_id = $1 AND id = $2 AND status = 'active'
		RETURNING `+credentialBindingMetadataColumns,
		organizationID, id, revokedAt.UTC()))
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return CredentialBinding{}, ErrNotFound
		}
		return CredentialBinding{}, fmt.Errorf("revoke credential binding: %w", err)
	}
	return revoked, nil
}

func (r *PostgresRepository) PurgeOrganizationCredentialBindings(ctx context.Context, organizationID string) (int, error) {
	tag, err := r.pool.Exec(ctx, `DELETE FROM credential_bindings WHERE organization_id = $1`, organizationID)
	if err != nil {
		return 0, fmt.Errorf("purge credential bindings: %w", err)
	}
	return int(tag.RowsAffected()), nil
}
