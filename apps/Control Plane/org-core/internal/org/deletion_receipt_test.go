package org

import "testing"

func TestValidateDeletionReceiptRejectsSemanticFailure(t *testing.T) {
	if err := validateDeletionReceipt([]byte(`{"success":false,"error":"dependent row remains","org_id":"org-1"}`)); err == nil {
		t.Fatal("semantic GDPR failure was accepted")
	}
}

func TestValidateDeletionReceiptAcceptsSuccess(t *testing.T) {
	if err := validateDeletionReceipt([]byte(`{"success":true,"org_id":"org-1"}`)); err != nil {
		t.Fatalf("successful GDPR receipt rejected: %v", err)
	}
}

func TestValidateDeletionReceiptRejectsMalformedPayload(t *testing.T) {
	if err := validateDeletionReceipt([]byte(`not-json`)); err == nil {
		t.Fatal("malformed GDPR receipt was accepted")
	}
}
