package db

import (
	"strings"
	"testing"
)

func TestActionReceiptAttestationMigration0008RemainsImmutable(t *testing.T) {
	raw, err := migrationFS.ReadFile("migrations/0008_action_receipt_attestations.sql")
	if err != nil {
		t.Fatalf("read migration error: %v", err)
	}
	sql := strings.ToLower(string(raw))
	for _, required := range []string{
		"attestation_issuer", "attestation_kid", "authorization_kind", "authorization_id",
		"approval_id", "action_id", "actor_id", "attestation_jti", "payload_sha256",
		"status in ('pending', 'executing', 'completed', 'unknown')",
		"unique", "attestation_issuer, organization_id, authorization_id",
		"authorization_kind = 'human_intent' and approval_id = ''",
		"authorization_kind = 'human_approved_ai_action' and approval_id <> ''",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	for _, strengthenedRelationship := range []string{"action_id = authorization_id", "approval_id = action_id"} {
		if strings.Contains(sql, strengthenedRelationship) {
			t.Fatalf("recorded migration 0008 must retain its weaker baseline; found %q", strengthenedRelationship)
		}
	}
	for _, forbidden := range []string{"message_body", "request_body", "params_json", "provider_response"} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("migration persists forbidden content field %q", forbidden)
		}
	}
}

func TestActionReceiptAuthorizationMigration0009AuditsThenEnforces(t *testing.T) {
	raw, err := migrationFS.ReadFile("migrations/0009_action_receipt_authorization_relationships.sql")
	if err != nil {
		t.Fatalf("read migration error: %v", err)
	}
	sql := strings.ToLower(string(raw))
	for _, required := range []string{
		"lock table integration_action_receipts in share row exclusive mode",
		"select count(*)", "attestation_issuer is distinct from ''", "is not true", "mismatched_receipt_count",
		"cannot strengthen provider-write authorization relationships",
		"drop constraint if exists integration_action_receipts_attestation_shape_check",
		"not valid", "validate constraint integration_action_receipts_attestation_shape_check",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}

	auditAt := strings.Index(sql, "select count(*)")
	dropAt := strings.Index(sql, "drop constraint if exists integration_action_receipts_attestation_shape_check")
	if auditAt < 0 || dropAt < 0 || auditAt >= dropAt {
		t.Fatal("migration must audit existing attested receipts before replacing the old constraint")
	}
	constraintAt := strings.Index(sql, "add constraint integration_action_receipts_attestation_shape_check")
	validateAt := strings.Index(sql, "validate constraint integration_action_receipts_attestation_shape_check")
	if constraintAt < 0 || validateAt < 0 || constraintAt >= validateAt {
		t.Fatal("migration must add the strengthened constraint before validating it")
	}
	constraintSQL := sql[constraintAt:validateAt]
	for _, required := range []string{"action_id = authorization_id", "approval_id = action_id", "not valid"} {
		if !strings.Contains(constraintSQL, required) {
			t.Fatalf("strengthened constraint missing %q", required)
		}
	}
	exceptionAt := strings.Index(sql, "raise exception using")
	if exceptionAt < 0 {
		t.Fatal("migration must raise a bounded audit error for mismatched legacy rows")
	}
	exceptionEndOffset := strings.Index(sql[exceptionAt:], "end if")
	if exceptionEndOffset < 0 {
		t.Fatal("migration must bound its audit exception block")
	}
	exceptionEndAt := exceptionAt + exceptionEndOffset
	exceptionSQL := sql[exceptionAt:exceptionEndAt]
	for _, forbiddenIdentifier := range []string{
		"authorization_id", "approval_id", "action_id", "actor_id", "attestation_jti", "payload_sha256",
	} {
		if strings.Contains(exceptionSQL, forbiddenIdentifier) {
			t.Fatalf("migration audit error must report only aggregate count; found %q", forbiddenIdentifier)
		}
	}
	for _, forbidden := range []string{
		"update integration_action_receipts", "message_body", "request_body", "params_json", "provider_response",
	} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("migration must not rewrite or report receipt content; found %q", forbidden)
		}
	}
}
