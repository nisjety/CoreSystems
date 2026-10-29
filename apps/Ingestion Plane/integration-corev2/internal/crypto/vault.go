package crypto

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"encoding/base64"
	"fmt"
	"io"
	"strings"
)

const ciphertextPrefix = "v1:"

type Vault struct {
	gcm cipher.AEAD
}

func NewVault(key []byte) (*Vault, error) {
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, fmt.Errorf("create cipher: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("create gcm: %w", err)
	}
	return &Vault{gcm: gcm}, nil
}

func (v *Vault) Encrypt(plaintext string, additionalData []byte) (string, error) {
	nonce := make([]byte, v.gcm.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return "", fmt.Errorf("generate nonce: %w", err)
	}
	sealed := v.gcm.Seal(nil, nonce, []byte(plaintext), additionalData)
	payload := append(nonce, sealed...)
	return ciphertextPrefix + base64.StdEncoding.EncodeToString(payload), nil
}

func (v *Vault) Decrypt(ciphertext string, additionalData []byte) (string, error) {
	encoded := strings.TrimPrefix(ciphertext, ciphertextPrefix)
	payload, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil {
		return "", fmt.Errorf("decode ciphertext: %w", err)
	}
	nonceSize := v.gcm.NonceSize()
	if len(payload) <= nonceSize {
		return "", fmt.Errorf("ciphertext too short")
	}
	nonce := payload[:nonceSize]
	sealed := payload[nonceSize:]
	plaintext, err := v.gcm.Open(nil, nonce, sealed, additionalData)
	if err != nil {
		return "", fmt.Errorf("decrypt ciphertext: %w", err)
	}
	return string(plaintext), nil
}
