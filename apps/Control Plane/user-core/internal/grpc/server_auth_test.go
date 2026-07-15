package grpc

import (
	"context"
	"os"
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

const (
	authPrimaryToken   = "auth-primary-0123456789abcdef0123456789abcdef"
	authSecondaryToken = "auth-secondary-0123456789abcdef0123456789abcdef"
	documentsToken     = "documents-api-0123456789abcdef0123456789abcdef"
)

func grpcCredentialsJSON() string {
	return `[
		{"credentialId":"auth-core-2026-07-a","principal":"auth-core","audience":"user-core-grpc","token":"` + authPrimaryToken + `","methods":["/user.v1.UserService/CreateUser","/user.v1.UserService/GetUser"]},
		{"credentialId":"auth-core-2026-07-b","principal":"auth-core","audience":"user-core-grpc","token":"` + authSecondaryToken + `","methods":["/user.v1.UserService/CreateUser","/user.v1.UserService/GetUser"]},
		{"credentialId":"documents-api-2026-07","principal":"documents-api","audience":"user-core-grpc","token":"` + documentsToken + `","methods":["/user.v1.DocumentAccessService/CheckDocumentAccess"]}
	]`
}

func serviceMetadata(credentialID, principal, token string) context.Context {
	return metadata.NewIncomingContext(context.Background(), metadata.Pairs(
		serviceCredentialIDMetadataKey, credentialID,
		servicePrincipalMetadataKey, principal,
		serviceAuthMetadataKey, token,
	))
}

func TestAuthorizeGRPCServiceCredentialRequiresExactTupleAndMethod(t *testing.T) {
	credentials, err := parseGRPCServiceCredentials(grpcCredentialsJSON())
	if err != nil {
		t.Fatalf("parseGRPCServiceCredentials() error = %v", err)
	}

	tests := []struct {
		name   string
		ctx    context.Context
		method string
		code   codes.Code
	}{
		{
			name:   "exact tuple and method",
			ctx:    serviceMetadata("auth-core-2026-07-a", "auth-core", authPrimaryToken),
			method: "/user.v1.UserService/CreateUser",
			code:   codes.OK,
		},
		{
			name:   "cross principal credential id is denied",
			ctx:    serviceMetadata("auth-core-2026-07-a", "documents-api", authPrimaryToken),
			method: "/user.v1.UserService/CreateUser",
			code:   codes.Unauthenticated,
		},
		{
			name:   "right tuple cannot call another method",
			ctx:    serviceMetadata("documents-api-2026-07", "documents-api", documentsToken),
			method: "/user.v1.DocumentAccessService/GrantDocumentAccess",
			code:   codes.PermissionDenied,
		},
		{
			name:   "wrong token",
			ctx:    serviceMetadata("auth-core-2026-07-a", "auth-core", authSecondaryToken),
			method: "/user.v1.UserService/CreateUser",
			code:   codes.Unauthenticated,
		},
		{
			name: "legacy shared key metadata is ignored",
			ctx: metadata.NewIncomingContext(context.Background(), metadata.Pairs(
				"x-internal-api-key", authPrimaryToken,
			)),
			method: "/user.v1.UserService/CreateUser",
			code:   codes.Unauthenticated,
		},
		{
			name:   "missing metadata",
			ctx:    context.Background(),
			method: "/user.v1.UserService/CreateUser",
			code:   codes.Unauthenticated,
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			err := authorizeGRPCServiceCredential(test.ctx, test.method, credentials)
			if test.code == codes.OK {
				if err != nil {
					t.Fatalf("authorizeGRPCServiceCredential() error = %v", err)
				}
				return
			}
			if got := status.Code(err); got != test.code {
				t.Fatalf("status.Code(authorizeGRPCServiceCredential()) = %s, want %s", got, test.code)
			}
		})
	}
}

func TestGRPCServiceCredentialRotationOverlapAndRetirement(t *testing.T) {
	overlap, err := parseGRPCServiceCredentials(grpcCredentialsJSON())
	if err != nil {
		t.Fatalf("parse overlap registry: %v", err)
	}
	method := "/user.v1.UserService/GetUser"
	for _, tuple := range []struct {
		id    string
		token string
	}{
		{id: "auth-core-2026-07-a", token: authPrimaryToken},
		{id: "auth-core-2026-07-b", token: authSecondaryToken},
	} {
		if err := authorizeGRPCServiceCredential(serviceMetadata(tuple.id, "auth-core", tuple.token), method, overlap); err != nil {
			t.Fatalf("overlap credential %s was denied: %v", tuple.id, err)
		}
	}

	retired, err := parseGRPCServiceCredentials(`[{"credentialId":"auth-core-2026-07-b","principal":"auth-core","audience":"user-core-grpc","token":"` + authSecondaryToken + `","methods":["/user.v1.UserService/GetUser"]}]`)
	if err != nil {
		t.Fatalf("parse retired registry: %v", err)
	}
	if got := status.Code(authorizeGRPCServiceCredential(serviceMetadata("auth-core-2026-07-a", "auth-core", authPrimaryToken), method, retired)); got != codes.Unauthenticated {
		t.Fatalf("retired credential status = %s, want %s", got, codes.Unauthenticated)
	}
	if err := authorizeGRPCServiceCredential(serviceMetadata("auth-core-2026-07-b", "auth-core", authSecondaryToken), method, retired); err != nil {
		t.Fatalf("replacement credential was denied: %v", err)
	}
}

func TestParseGRPCServiceCredentialsFailsClosed(t *testing.T) {
	tests := []struct {
		name string
		raw  string
	}{
		{name: "missing", raw: ""},
		{name: "malformed", raw: "["},
		{name: "wrong audience", raw: `[{"credentialId":"auth-core-a","principal":"auth-core","audience":"user-core","token":"` + authPrimaryToken + `","methods":["/user.v1.UserService/GetUser"]}]`},
		{name: "short token", raw: `[{"credentialId":"auth-core-a","principal":"auth-core","audience":"user-core-grpc","token":"short","methods":["/user.v1.UserService/GetUser"]}]`},
		{name: "wildcard method", raw: `[{"credentialId":"auth-core-a","principal":"auth-core","audience":"user-core-grpc","token":"` + authPrimaryToken + `","methods":["*"]}]`},
		{name: "unknown method", raw: `[{"credentialId":"auth-core-a","principal":"auth-core","audience":"user-core-grpc","token":"` + authPrimaryToken + `","methods":["/user.v1.UserService/DoesNotExist"]}]`},
		{name: "duplicate id", raw: `[{"credentialId":"auth-core-a","principal":"auth-core","audience":"user-core-grpc","token":"` + authPrimaryToken + `","methods":["/user.v1.UserService/GetUser"]},{"credentialId":"auth-core-a","principal":"auth-core","audience":"user-core-grpc","token":"` + authSecondaryToken + `","methods":["/user.v1.UserService/GetUser"]}]`},
		{name: "duplicate token", raw: `[{"credentialId":"auth-core-a","principal":"auth-core","audience":"user-core-grpc","token":"` + authPrimaryToken + `","methods":["/user.v1.UserService/GetUser"]},{"credentialId":"auth-core-b","principal":"auth-core","audience":"user-core-grpc","token":"` + authPrimaryToken + `","methods":["/user.v1.UserService/GetUser"]}]`},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if _, err := parseGRPCServiceCredentials(test.raw); err == nil {
				t.Fatal("parseGRPCServiceCredentials() error = nil, want failure")
			}
		})
	}
}

func TestLoadGRPCServiceCredentialsRequiresFileInProduction(t *testing.T) {
	_, err := loadGRPCServiceCredentials(grpcCredentialEnvironment{
		Environment:       "production",
		InlineCredentials: grpcCredentialsJSON(),
	})
	if err == nil {
		t.Fatal("loadGRPCServiceCredentials() error = nil, want production file requirement")
	}

	path := t.TempDir() + "/credentials.json"
	if err := os.WriteFile(path, []byte(grpcCredentialsJSON()), 0600); err != nil {
		t.Fatalf("write fixture: %v", err)
	}
	credentials, err := loadGRPCServiceCredentials(grpcCredentialEnvironment{
		Environment:     "production",
		CredentialsFile: path,
	})
	if err != nil {
		t.Fatalf("loadGRPCServiceCredentials() error = %v", err)
	}
	if len(credentials) != 3 {
		t.Fatalf("credential count = %d, want 3", len(credentials))
	}
}
