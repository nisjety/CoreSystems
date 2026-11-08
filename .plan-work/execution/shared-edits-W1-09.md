## WS06-S5
shared-edit apps/Control Plane/audit-core/internal/chain/sign.go: add `func (k *KeyRing) SignPayload(payload []byte) (string, []byte, error) { return k.sign(payload) }` and `func (k *KeyRing) VerifyPayload(keyID string, payload, signature []byte) error { return k.verify(keyID, payload, signature) }` (KeyRing.sign already returns ErrNoSigningKey when it cannot sign, which store.SignManifest treats as 'serve unsigned'). No other change is needed: store.payloadSigner picks the key ring up by interface assertion. Then the manifest on a signing deployment is signed with magic VAM1.

## WS06-S5
shared-edit docker-compose.yml: in the audit-core AUDIT_CORE_SERVICE_CREDENTIALS block add scope "audit:compliance:read:self" to the verevon-gateway principal only (never another principal: the parser now rejects it). Deploy order: audit-core image first, compose after (an old image rejects the unknown scope and will not boot); roll via scripts/run-control-plane.sh, never bare docker compose build. Update run-control-plane.sh / validate-release-credentials.sh / compose-contract tests if they pin that scope list.

