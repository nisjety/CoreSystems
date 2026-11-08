package grpc

import (
	"strings"
	"testing"
)

// WS14-S2: resource_grants has one reviewed write surface (the HTTP facade in
// owner-asserted mode). The dormant DocumentAccessService grant/revoke methods
// skip the owner check, so a credential may hold them only for a principal that
// is named in code, and no principal is named today.

func credentialFor(principal string, methods ...string) string {
	quoted := make([]string, len(methods))
	for i, method := range methods {
		quoted[i] = `"` + method + `"`
	}
	return `[{"credentialId":"` + principal + `-2026-11","principal":"` + principal + `","audience":"user-core-grpc","token":"` +
		principal + `-0123456789abcdef0123456789abcdef","methods":[` + strings.Join(quoted, ",") + `]}]`
}

func TestGrantWriterMethodsAreRefusedForUnnamedPrincipalsAtRegistryLoad(t *testing.T) {
	if len(grpcGrantWriterPrincipals) != 0 {
		t.Fatalf("no principal may be named as a grant writer today, got %v; adding one is a reviewed change to this test", grpcGrantWriterPrincipals)
	}
	for _, method := range []string{
		"/user.v1.DocumentAccessService/GrantDocumentAccess",
		"/user.v1.DocumentAccessService/RevokeDocumentAccess",
	} {
		for _, principal := range []string{"documents-api", "auth-core", "verevon-gateway", "anyone-else"} {
			t.Run(principal+" "+method, func(t *testing.T) {
				_, err := parseGRPCServiceCredentials(credentialFor(principal, method))
				if err == nil {
					t.Fatal("a grant-writer method on an unnamed principal must be refused at load")
				}
				if !strings.Contains(err.Error(), "resource-grant writer method to an unnamed principal") {
					t.Fatalf("error = %v", err)
				}
			})
		}
	}
}

func TestGrantWriterMethodRefusalHoldsEvenWhenListedAmongPermittedMethods(t *testing.T) {
	_, err := parseGRPCServiceCredentials(credentialFor("documents-api",
		"/user.v1.DocumentAccessService/CheckDocumentAccess",
		"/user.v1.DocumentAccessService/GrantDocumentAccess"))
	if err == nil || !strings.Contains(err.Error(), "unnamed principal") {
		t.Fatalf("one writer method among readers must still refuse the whole registry: %v", err)
	}
}

func TestReadOnlyMethodsStillLoadForTheSamePrincipal(t *testing.T) {
	credentials, err := parseGRPCServiceCredentials(credentialFor("documents-api", "/user.v1.DocumentAccessService/CheckDocumentAccess"))
	if err != nil || len(credentials) != 1 {
		t.Fatalf("a read-only credential must load: %v %v", credentials, err)
	}
}

func TestGrantWriterMethodsAreRealRegisteredMethods(t *testing.T) {
	// A rename of the gRPC method would silently turn the guard into dead code:
	// every method the audit lists must be a method the registry allows today.
	allowed := allowedGRPCMethodSet()
	for method := range grpcGrantWriterMethods {
		if _, ok := allowed[method]; !ok {
			t.Errorf("grant-writer method %q is not an allowed gRPC method; the audit list is stale", method)
		}
	}
}
