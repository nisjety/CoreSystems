package grpc

import (
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/json"
	"fmt"
	"os"
	"regexp"
	"strings"

	pb "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/proto/user/v1"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

const (
	grpcServiceAudience            = "user-core-grpc"
	serviceCredentialIDMetadataKey = "x-service-credential-id"
	servicePrincipalMetadataKey    = "x-service-principal"
	serviceAuthMetadataKey         = "x-service-auth"
	maximumGRPCCredentialFileBytes = 1024 * 1024
)

var grpcPolicyIdentifier = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{1,127}$`)

type grpcServiceCredential struct {
	CredentialID string   `json:"credentialId"`
	Principal    string   `json:"principal"`
	Audience     string   `json:"audience"`
	Token        string   `json:"token"`
	Methods      []string `json:"methods"`
}

type grpcCredentialEnvironment struct {
	Environment       string
	InlineCredentials string
	CredentialsFile   string
}

func configuredGRPCCredentialEnvironment() grpcCredentialEnvironment {
	return grpcCredentialEnvironment{
		Environment:       strings.TrimSpace(os.Getenv("ENVIRONMENT")),
		InlineCredentials: os.Getenv("USER_CORE_GRPC_SERVICE_CREDENTIALS"),
		CredentialsFile:   strings.TrimSpace(os.Getenv("USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE")),
	}
}

func allowedGRPCMethodSet() map[string]struct{} {
	methods := make(map[string]struct{}, len(pb.UserService_ServiceDesc.Methods)+len(pb.DocumentAccessService_ServiceDesc.Methods))
	for _, method := range pb.UserService_ServiceDesc.Methods {
		methods["/"+pb.UserService_ServiceDesc.ServiceName+"/"+method.MethodName] = struct{}{}
	}
	for _, method := range pb.DocumentAccessService_ServiceDesc.Methods {
		methods["/"+pb.DocumentAccessService_ServiceDesc.ServiceName+"/"+method.MethodName] = struct{}{}
	}
	return methods
}

func exactGRPCCredentialFields(raw json.RawMessage) bool {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return false
	}
	if len(fields) != 5 {
		return false
	}
	for _, name := range []string{"credentialId", "principal", "audience", "token", "methods"} {
		if _, ok := fields[name]; !ok {
			return false
		}
	}
	return true
}

func validGRPCCredentialToken(token string) bool {
	if token != strings.TrimSpace(token) || len(token) < 32 {
		return false
	}
	lower := strings.ToLower(token)
	for _, prefix := range []string{"test", "placeholder", "change-me", "replace-with", "your-"} {
		if strings.HasPrefix(lower, prefix) {
			return false
		}
	}
	return true
}

func parseGRPCServiceCredentials(raw string) ([]grpcServiceCredential, error) {
	var entries []json.RawMessage
	if err := json.Unmarshal([]byte(strings.TrimSpace(raw)), &entries); err != nil {
		return nil, fmt.Errorf("decode USER_CORE_GRPC_SERVICE_CREDENTIALS: %w", err)
	}
	if len(entries) == 0 {
		return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS must be a non-empty array")
	}

	allowedMethods := allowedGRPCMethodSet()
	credentialIDs := make(map[string]struct{}, len(entries))
	tokens := make(map[string]struct{}, len(entries))
	credentials := make([]grpcServiceCredential, 0, len(entries))
	for _, rawEntry := range entries {
		if !exactGRPCCredentialFields(rawEntry) {
			return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS contains an invalid principal policy")
		}
		var credential grpcServiceCredential
		if err := json.Unmarshal(rawEntry, &credential); err != nil {
			return nil, fmt.Errorf("decode USER_CORE_GRPC_SERVICE_CREDENTIALS entry: %w", err)
		}
		if credential.CredentialID != strings.TrimSpace(credential.CredentialID) ||
			credential.Principal != strings.TrimSpace(credential.Principal) ||
			!grpcPolicyIdentifier.MatchString(credential.CredentialID) ||
			!grpcPolicyIdentifier.MatchString(credential.Principal) ||
			credential.Audience != grpcServiceAudience ||
			!validGRPCCredentialToken(credential.Token) ||
			len(credential.Methods) == 0 {
			return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS contains an invalid principal policy")
		}
		if _, duplicate := credentialIDs[credential.CredentialID]; duplicate {
			return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS contains a duplicate credential id")
		}
		if _, duplicate := tokens[credential.Token]; duplicate {
			return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS contains a duplicate token")
		}
		methodSet := make(map[string]struct{}, len(credential.Methods))
		methods := make([]string, 0, len(credential.Methods))
		for _, method := range credential.Methods {
			if method != strings.TrimSpace(method) {
				return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS contains an invalid method")
			}
			if _, ok := allowedMethods[method]; !ok {
				return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS contains an unknown method")
			}
			if _, duplicate := methodSet[method]; duplicate {
				return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS contains a duplicate method")
			}
			methodSet[method] = struct{}{}
			methods = append(methods, method)
		}

		credentialIDs[credential.CredentialID] = struct{}{}
		tokens[credential.Token] = struct{}{}
		credential.Methods = methods
		credentials = append(credentials, credential)
	}
	return credentials, nil
}

func loadGRPCServiceCredentials(environment grpcCredentialEnvironment) ([]grpcServiceCredential, error) {
	if environment.CredentialsFile != "" {
		info, err := os.Stat(environment.CredentialsFile)
		if err != nil {
			return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE could not be read")
		}
		if info.Size() > maximumGRPCCredentialFileBytes {
			return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE is too large")
		}
		raw, err := os.ReadFile(environment.CredentialsFile)
		if err != nil {
			return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE could not be read")
		}
		return parseGRPCServiceCredentials(string(raw))
	}

	runtime := strings.ToLower(strings.TrimSpace(environment.Environment))
	if runtime == "production" {
		return nil, fmt.Errorf("USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE is required in production")
	}
	return parseGRPCServiceCredentials(environment.InlineCredentials)
}

// ValidateGRPCServiceCredentialRegistry verifies deployment-owned gRPC
// principals before any User Core listener is opened.
func ValidateGRPCServiceCredentialRegistry() error {
	_, err := loadGRPCServiceCredentials(configuredGRPCCredentialEnvironment())
	return err
}

func singleMetadataValue(ctx context.Context, name string) string {
	md, ok := metadata.FromIncomingContext(ctx)
	if !ok {
		return ""
	}
	values := md.Get(name)
	if len(values) != 1 || values[0] == "" || values[0] != strings.TrimSpace(values[0]) {
		return ""
	}
	return values[0]
}

func secureGRPCTokenEqual(expected, received string) bool {
	expectedDigest := sha256.Sum256([]byte(expected))
	receivedDigest := sha256.Sum256([]byte(received))
	return subtle.ConstantTimeCompare(expectedDigest[:], receivedDigest[:]) == 1
}

func authorizeGRPCServiceCredential(ctx context.Context, method string, credentials []grpcServiceCredential) error {
	credentialID := singleMetadataValue(ctx, serviceCredentialIDMetadataKey)
	principal := singleMetadataValue(ctx, servicePrincipalMetadataKey)
	token := singleMetadataValue(ctx, serviceAuthMetadataKey)

	var matched *grpcServiceCredential
	for index := range credentials {
		if credentials[index].CredentialID == credentialID {
			matched = &credentials[index]
			break
		}
	}
	expectedToken := "invalid-user-core-grpc-service-credential"
	if matched != nil {
		expectedToken = matched.Token
	}
	if credentialID == "" || principal == "" || token == "" || matched == nil ||
		matched.Principal != principal || !secureGRPCTokenEqual(expectedToken, token) {
		return status.Error(codes.Unauthenticated, "User Core gRPC service credential is invalid")
	}
	for _, allowed := range matched.Methods {
		if allowed == method {
			return nil
		}
	}
	return status.Error(codes.PermissionDenied, "User Core gRPC service principal lacks method authority")
}
