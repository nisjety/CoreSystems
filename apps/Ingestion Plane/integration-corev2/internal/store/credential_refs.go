package store

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"
)

// Credential reference reads (W4-CC-4, ADR-NEXT-sandbox-credential-injection sections 2 and 7).
//
// Both reads are scoped by organisation: a binding id from another organisation is ErrNotFound.
// GetCredentialBinding is the seal route's read and never selects the ciphertext.
// GetCredentialBindingForRedeem is the only read that returns the sealed secret, and only to the
// redeem route, which opens it through the vault and returns the plaintext to the proxy principal.

// GetCredentialBinding returns an organisation's binding as metadata only, whatever its status.
func (r *MemoryRepository) GetCredentialBinding(_ context.Context, organizationID, id string) (CredentialBinding, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	stored, ok := r.credentialBindings[id]
	if !ok || stored.OrganizationID != organizationID {
		return CredentialBinding{}, ErrNotFound
	}
	return stored.metadata(), nil
}

// GetCredentialBindingForRedeem returns an organisation's binding with its ciphertext, whatever its
// status. A revoked binding has no ciphertext left.
func (r *MemoryRepository) GetCredentialBindingForRedeem(_ context.Context, organizationID, id string) (CredentialBinding, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	stored, ok := r.credentialBindings[id]
	if !ok || stored.OrganizationID != organizationID {
		return CredentialBinding{}, ErrNotFound
	}
	return cloneCredentialBinding(stored), nil
}

// GetCredentialBinding returns an organisation's binding as metadata only, whatever its status.
func (r *PostgresRepository) GetCredentialBinding(ctx context.Context, organizationID, id string) (CredentialBinding, error) {
	binding, err := scanCredentialBinding(r.pool.QueryRow(ctx, `
		SELECT `+credentialBindingMetadataColumns+`
		FROM credential_bindings
		WHERE organization_id = $1 AND id = $2
	`, organizationID, id))
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return CredentialBinding{}, ErrNotFound
		}
		return CredentialBinding{}, fmt.Errorf("get credential binding: %w", err)
	}
	return binding, nil
}

// GetCredentialBindingForRedeem returns an organisation's binding with its ciphertext, whatever its
// status. The migration's CHECK keeps a revoked binding's ciphertext empty.
func (r *PostgresRepository) GetCredentialBindingForRedeem(ctx context.Context, organizationID, id string) (CredentialBinding, error) {
	var binding CredentialBinding
	err := r.pool.QueryRow(ctx, `
		SELECT `+credentialBindingMetadataColumns+`, secret_ciphertext
		FROM credential_bindings
		WHERE organization_id = $1 AND id = $2
	`, organizationID, id).Scan(
		&binding.ID, &binding.OrganizationID, &binding.CreatedByUserID, &binding.Kind, &binding.Host, &binding.HeaderName,
		&binding.AllowedMethods, &binding.AllowedPathPrefixes, &binding.KeyVersion, &binding.Status, &binding.CreatedAt, &binding.RevokedAt,
		&binding.SecretCiphertext,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return CredentialBinding{}, ErrNotFound
		}
		return CredentialBinding{}, fmt.Errorf("get credential binding for redeem: %w", err)
	}
	binding.AllowedMethods = nonNilStringSlice(binding.AllowedMethods)
	binding.AllowedPathPrefixes = nonNilStringSlice(binding.AllowedPathPrefixes)
	return binding, nil
}
