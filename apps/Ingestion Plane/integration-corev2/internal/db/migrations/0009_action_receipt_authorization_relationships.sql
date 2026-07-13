-- Forward-only hardening for installations that already recorded 0008.
-- Do not rewrite legacy receipt metadata automatically: mismatches require an
-- operator audit because completed or unknown provider effects are irreversible.

LOCK TABLE integration_action_receipts IN SHARE ROW EXCLUSIVE MODE;

DO $$
DECLARE
  mismatched_receipt_count BIGINT;
BEGIN
  SELECT COUNT(*)
    INTO mismatched_receipt_count
    FROM integration_action_receipts
   WHERE attestation_issuer IS DISTINCT FROM ''
     AND (
       (
         (authorization_kind = 'human_intent' AND approval_id = '' AND action_id = authorization_id) OR
         (authorization_kind = 'human_approved_ai_action' AND approval_id = action_id)
       ) IS NOT TRUE
     );

  IF mismatched_receipt_count > 0 THEN
    RAISE EXCEPTION USING
      ERRCODE = 'check_violation',
      MESSAGE = 'cannot strengthen provider-write authorization relationships',
      DETAIL = format('%s attested receipt row(s) have mismatched authorization identifiers', mismatched_receipt_count),
      HINT = 'audit and reconcile the affected receipts before rerunning this migration; do not rewrite completed or unknown provider effects blindly';
  END IF;
END
$$;

ALTER TABLE integration_action_receipts
  DROP CONSTRAINT IF EXISTS integration_action_receipts_attestation_shape_check;

ALTER TABLE integration_action_receipts
  ADD CONSTRAINT integration_action_receipts_attestation_shape_check
    CHECK (
      (
        attestation_issuer = '' AND attestation_kid = '' AND
        authorization_kind = '' AND authorization_id = '' AND
        approval_id = '' AND action_id = '' AND actor_id = '' AND
        attestation_jti = '' AND payload_sha256 = ''
      ) OR (
        attestation_issuer = 'conversation-core' AND
        attestation_kid <> '' AND authorization_id <> '' AND
        action_id <> '' AND actor_id <> '' AND attestation_jti <> '' AND
        payload_sha256 ~ '^[0-9a-f]{64}$' AND
        (
          (authorization_kind = 'human_intent' AND approval_id = '' AND action_id = authorization_id) OR
          (authorization_kind = 'human_approved_ai_action' AND approval_id = action_id)
        )
      )
    ) NOT VALID;

ALTER TABLE integration_action_receipts
  VALIDATE CONSTRAINT integration_action_receipts_attestation_shape_check;
