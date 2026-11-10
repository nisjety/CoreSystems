package api

import (
	"context"
	"errors"
	"strings"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/store"
)

// custom_rest.call on the actions surface (dark, CUSTOM_REST_CONNECTORS_ENABLED). W4-CC-6.
//
// A connection of provider custom_rest names one credential binding (W4-CC-3) in its
// providerContext. The call itself runs in internal/actions/custom_rest.go; this file holds what
// the HTTP layer decides around it:
//
//   - capability and approval by method: GET and HEAD need custom_rest.read and run as reads; every
//     other method needs custom_rest.write and runs as an attested, idempotent, approved write. A
//     missing or unknown method counts as a write, so it can never lower the bar.
//   - the binding is read for this call only, scoped to the connection's organisation, opened with
//     the vault, and handed to the executor. It is never cached, returned or logged, and the
//     opening writes a connection.credential.redeemed row first (fail closed).
//
// Zero data retention: a custom_rest connection has no ZDR assessment record, so every reader
// treats it as cannot_verify and no ZDR claim is made for it.

var (
	// errCustomRESTBindingUnavailable is the answer when the connection's binding is not named,
	// not found, revoked, not a REST credential, or cannot be opened under the current key.
	errCustomRESTBindingUnavailable = errors.New("the connection's credential binding is not available")
	// errCustomRESTAuditFailed is the answer when the redemption row could not be written. No
	// secret leaves the store.
	errCustomRESTAuditFailed = errors.New("the credential redemption could not be audited")
)

// customRESTCallIsWrite reports whether a custom_rest.call must run as an approved write: every
// method except GET and HEAD.
func customRESTCallIsWrite(providerKey string, body actionBody) bool {
	return providerKey == actions.CustomRESTProviderKey &&
		actions.IsCustomRESTOperation(body.Operation) &&
		!actions.CustomRESTIsRead(body.Params)
}

// customRESTCapability returns the capability custom_rest.call needs for its method. It is always
// sensitive, so an empty capability list never passes it. An operation other than custom_rest.call
// has no capability and is unsupported.
func customRESTCapability(body actionBody) (string, bool) {
	if !actions.IsCustomRESTOperation(body.Operation) {
		return "", false
	}
	if actions.CustomRESTIsRead(body.Params) {
		return actions.CustomRESTCapabilityRead, true
	}
	return actions.CustomRESTCapabilityWrite, true
}

// openCustomRESTBinding reads the connection's binding, opens its secret and records the
// redemption. callID is the write's idempotency key, empty for a read.
func openCustomRESTBinding(ctx context.Context, cfg ServerConfig, connection store.Connection, callID string) (*actions.CustomRESTBinding, error) {
	if connection.DeletedAt != nil || connection.Status != "active" {
		return nil, store.ErrNotFound
	}
	if cfg.Repo == nil || cfg.CredentialSealer == nil {
		return nil, errActionUnavailable
	}
	bindingID := strings.TrimSpace(connection.ProviderContext[actions.CustomRESTBindingContextKey])
	if bindingID == "" {
		return nil, errCustomRESTBindingUnavailable
	}
	binding, err := cfg.Repo.GetCredentialBindingForRedeem(ctx, connection.OrganizationID, bindingID)
	if err != nil {
		if errors.Is(err, store.ErrNotFound) {
			return nil, errCustomRESTBindingUnavailable
		}
		return nil, err
	}
	if binding.Status != store.CredentialBindingStatusActive {
		return nil, errCustomRESTBindingUnavailable
	}
	if binding.Kind != store.CredentialBindingKindRESTAPIKey && binding.Kind != store.CredentialBindingKindRESTBearer {
		return nil, errCustomRESTBindingUnavailable
	}
	secret, err := cfg.CredentialSealer.Decrypt(string(binding.SecretCiphertext), []byte(binding.ID))
	if err != nil {
		// A ciphertext sealed under a retired key lands here and fails closed.
		return nil, errCustomRESTBindingUnavailable
	}
	callID = strings.TrimSpace(callID)
	if !credentialRefTokenValid(callID) {
		callID = actions.OperationCustomRESTCall
	}
	if err := auditCredentialRedemption(ctx, cfg, credentialRefAudit{
		OrganizationID: connection.OrganizationID,
		ConnectionID:   connection.ID,
		BindingID:      binding.ID,
		CallID:         callID,
		Host:           binding.Host,
	}, credentialRefOutcomeAllowed, credentialRefReasonOK); err != nil {
		return nil, errCustomRESTAuditFailed
	}
	return &actions.CustomRESTBinding{
		ID:                  binding.ID,
		Kind:                binding.Kind,
		Host:                binding.Host,
		HeaderName:          binding.HeaderName,
		AllowedMethods:      binding.AllowedMethods,
		AllowedPathPrefixes: binding.AllowedPathPrefixes,
		Secret:              secret,
	}, nil
}
