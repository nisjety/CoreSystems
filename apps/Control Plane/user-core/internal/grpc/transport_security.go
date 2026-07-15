package grpc

import (
	"crypto/tls"
	"fmt"
	"os"
	"strings"

	"google.golang.org/grpc/credentials"
)

const maximumGRPCTLSFileBytes = 1024 * 1024

type grpcTLSEnvironment struct {
	Environment     string
	CertificateFile string
	PrivateKeyFile  string
}

func configuredGRPCTLSEnvironment(environment string) grpcTLSEnvironment {
	return grpcTLSEnvironment{
		Environment:     environment,
		CertificateFile: strings.TrimSpace(os.Getenv("USER_CORE_GRPC_TLS_CERT_FILE")),
		PrivateKeyFile:  strings.TrimSpace(os.Getenv("USER_CORE_GRPC_TLS_KEY_FILE")),
	}
}

func readBoundedTLSFile(path, name string) ([]byte, error) {
	info, err := os.Stat(path)
	if err != nil || info.Size() <= 0 || info.Size() > maximumGRPCTLSFileBytes {
		return nil, fmt.Errorf("%s could not be read", name)
	}
	contents, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("%s could not be read", name)
	}
	return contents, nil
}

func loadGRPCServerTransportCredentials(environment grpcTLSEnvironment) (credentials.TransportCredentials, error) {
	certificateFile := strings.TrimSpace(environment.CertificateFile)
	privateKeyFile := strings.TrimSpace(environment.PrivateKeyFile)
	production := strings.EqualFold(strings.TrimSpace(environment.Environment), "production")
	if certificateFile == "" && privateKeyFile == "" {
		if production {
			return nil, fmt.Errorf("USER_CORE_GRPC_TLS_CERT_FILE and USER_CORE_GRPC_TLS_KEY_FILE are required in production")
		}
		return nil, nil
	}
	if certificateFile == "" || privateKeyFile == "" {
		return nil, fmt.Errorf("USER_CORE_GRPC_TLS_CERT_FILE and USER_CORE_GRPC_TLS_KEY_FILE must be configured together")
	}

	certificatePEM, err := readBoundedTLSFile(certificateFile, "USER_CORE_GRPC_TLS_CERT_FILE")
	if err != nil {
		return nil, err
	}
	privateKeyPEM, err := readBoundedTLSFile(privateKeyFile, "USER_CORE_GRPC_TLS_KEY_FILE")
	if err != nil {
		return nil, err
	}
	certificate, err := tls.X509KeyPair(certificatePEM, privateKeyPEM)
	if err != nil {
		return nil, fmt.Errorf("User Core gRPC TLS certificate and private key are invalid or mismatched")
	}
	return credentials.NewTLS(&tls.Config{
		Certificates: []tls.Certificate{certificate},
		MinVersion:   tls.VersionTLS13,
	}), nil
}
