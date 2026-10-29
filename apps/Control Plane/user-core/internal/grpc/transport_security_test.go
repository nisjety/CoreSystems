package grpc

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/pem"
	"math/big"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func writeServerCertificate(t *testing.T, directory string) (string, string) {
	t.Helper()
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate private key: %v", err)
	}
	template := x509.Certificate{
		SerialNumber: big.NewInt(1),
		Subject:      pkix.Name{CommonName: "user-core"},
		DNSNames:     []string{"user-core"},
		NotBefore:    time.Now().Add(-time.Minute),
		NotAfter:     time.Now().Add(time.Hour),
		KeyUsage:     x509.KeyUsageDigitalSignature | x509.KeyUsageKeyEncipherment,
		ExtKeyUsage:  []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
	}
	certificateDER, err := x509.CreateCertificate(rand.Reader, &template, &template, &privateKey.PublicKey, privateKey)
	if err != nil {
		t.Fatalf("create certificate: %v", err)
	}
	certificatePath := filepath.Join(directory, "server.crt")
	keyPath := filepath.Join(directory, "server.key")
	if err := os.WriteFile(certificatePath, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: certificateDER}), 0600); err != nil {
		t.Fatalf("write certificate: %v", err)
	}
	if err := os.WriteFile(keyPath, pem.EncodeToMemory(&pem.Block{Type: "RSA PRIVATE KEY", Bytes: x509.MarshalPKCS1PrivateKey(privateKey)}), 0600); err != nil {
		t.Fatalf("write private key: %v", err)
	}
	return certificatePath, keyPath
}

func TestLoadGRPCServerTransportCredentialsRequiresTLSInProduction(t *testing.T) {
	credentials, err := loadGRPCServerTransportCredentials(grpcTLSEnvironment{Environment: "production"})
	if err == nil || !strings.Contains(err.Error(), "USER_CORE_GRPC_TLS_CERT_FILE") {
		t.Fatalf("missing production TLS error = %v", err)
	}
	if credentials != nil {
		t.Fatal("missing production TLS returned credentials")
	}
}

func TestConfiguredGRPCTLSEnvironmentReadsBoundedFileLocations(t *testing.T) {
	t.Setenv("USER_CORE_GRPC_TLS_CERT_FILE", " /run/secrets/user-core.crt ")
	t.Setenv("USER_CORE_GRPC_TLS_KEY_FILE", " /run/secrets/user-core.key ")

	got := configuredGRPCTLSEnvironment("production")
	if got.Environment != "production" ||
		got.CertificateFile != "/run/secrets/user-core.crt" ||
		got.PrivateKeyFile != "/run/secrets/user-core.key" {
		t.Fatalf("configuredGRPCTLSEnvironment() = %+v", got)
	}
}

func TestLoadGRPCServerTransportCredentialsAllowsDevelopmentWithoutTLS(t *testing.T) {
	credentials, err := loadGRPCServerTransportCredentials(grpcTLSEnvironment{Environment: "development"})
	if err != nil {
		t.Fatalf("development transport error = %v", err)
	}
	if credentials != nil {
		t.Fatal("development transport unexpectedly enabled TLS")
	}
}

func TestLoadGRPCServerTransportCredentialsLoadsMatchingPair(t *testing.T) {
	certificatePath, keyPath := writeServerCertificate(t, t.TempDir())
	credentials, err := loadGRPCServerTransportCredentials(grpcTLSEnvironment{
		Environment:     "production",
		CertificateFile: certificatePath,
		PrivateKeyFile:  keyPath,
	})
	if err != nil {
		t.Fatalf("load production transport: %v", err)
	}
	if credentials == nil {
		t.Fatal("load production transport returned nil credentials")
	}
}

func TestLoadGRPCServerTransportCredentialsFailsClosed(t *testing.T) {
	certificatePath, keyPath := writeServerCertificate(t, t.TempDir())
	_, differentKeyPath := writeServerCertificate(t, t.TempDir())
	malformedCertificatePath := filepath.Join(t.TempDir(), "malformed.crt")
	if err := os.WriteFile(malformedCertificatePath, []byte("not a certificate"), 0600); err != nil {
		t.Fatalf("write malformed certificate: %v", err)
	}
	tests := []grpcTLSEnvironment{
		{Environment: "production", CertificateFile: certificatePath},
		{Environment: "production", PrivateKeyFile: keyPath},
		{Environment: "production", CertificateFile: certificatePath, PrivateKeyFile: differentKeyPath},
		{Environment: "production", CertificateFile: filepath.Join(t.TempDir(), "missing.crt"), PrivateKeyFile: keyPath},
		{Environment: "production", CertificateFile: malformedCertificatePath, PrivateKeyFile: keyPath},
	}
	for _, environment := range tests {
		if credentials, err := loadGRPCServerTransportCredentials(environment); err == nil || credentials != nil {
			t.Fatalf("loadGRPCServerTransportCredentials(%+v) = (%v, %v), want failure", environment, credentials, err)
		}
	}
}
