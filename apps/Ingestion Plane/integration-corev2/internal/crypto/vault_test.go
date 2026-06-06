package crypto

import "testing"

func TestVaultRoundTripUsesAdditionalData(t *testing.T) {
	vault, err := NewVault([]byte("12345678901234567890123456789012"))
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}

	ciphertext, err := vault.Encrypt("secret", []byte("org-1"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	plaintext, err := vault.Decrypt(ciphertext, []byte("org-1"))
	if err != nil {
		t.Fatalf("Decrypt error: %v", err)
	}
	if plaintext != "secret" {
		t.Fatalf("plaintext = %q, want secret", plaintext)
	}
	if _, err := vault.Decrypt(ciphertext, []byte("org-2")); err == nil {
		t.Fatalf("Decrypt with wrong additional data succeeded")
	}
}
