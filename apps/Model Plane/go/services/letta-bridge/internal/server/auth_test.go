package server

import (
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
)

func TestMemoryAuthorizerPinsTenantAndScopes(t *testing.T) {
	user := authctx.Principal{
		OrganizationID:         "org-a",
		ActorID:                "user-a",
		PrincipalType:          "user",
		RetentionPolicyPresent: true,
	}
	if err := MemoryAuthorizer(user, mpv1.MemoryService_SearchMemory_FullMethodName,
		&mpv1.SearchMemoryRequest{OrgId: "org-a"}); err != nil {
		t.Fatalf("own tenant read denied: %v", err)
	}
	if err := MemoryAuthorizer(user, mpv1.MemoryService_SearchMemory_FullMethodName,
		&mpv1.SearchMemoryRequest{OrgId: "org-b"}); err == nil {
		t.Fatal("cross-tenant read must be denied")
	}
	if err := MemoryAuthorizer(user, mpv1.MemoryService_IndexMemory_FullMethodName,
		&mpv1.IndexMemoryRequest{}); err == nil {
		t.Fatal("empty request tenant must not become a wildcard")
	}

	service := authctx.Principal{
		OrganizationID:         "org-a",
		ActorID:                "service:model-gateway",
		PrincipalType:          "service",
		Scopes:                 []string{"memory:read"},
		RetentionPolicyPresent: true,
	}
	if err := MemoryAuthorizer(service, mpv1.MemoryService_SearchMemory_FullMethodName,
		&mpv1.SearchMemoryRequest{OrgId: "org-a"}); err != nil {
		t.Fatalf("scoped service read denied: %v", err)
	}
	if err := MemoryAuthorizer(service, mpv1.MemoryService_IndexMemory_FullMethodName,
		&mpv1.IndexMemoryRequest{OrgId: "org-a"}); err == nil {
		t.Fatal("read-only service must not write memory")
	}
}

func TestMemoryAuthorizerAcceptsOnlyScopedSessionCoreMemoryOperations(t *testing.T) {
	principal := authctx.Principal{
		OrganizationID:         "org-a",
		ActorID:                "service:session-core",
		PrincipalType:          "service",
		Scopes:                 []string{"memory:read", "memory:write"},
		RetentionPolicyPresent: true,
	}

	for _, test := range []struct {
		name    string
		method  string
		request any
	}{
		{
			name:    "semantic read",
			method:  mpv1.MemoryService_SearchMemory_FullMethodName,
			request: &mpv1.SearchMemoryRequest{OrgId: "org-a"},
		},
		{
			name:    "dreaming write",
			method:  mpv1.MemoryService_IndexMemory_FullMethodName,
			request: &mpv1.IndexMemoryRequest{OrgId: "org-a"},
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			if err := MemoryAuthorizer(principal, test.method, test.request); err != nil {
				t.Fatalf("session-core operation denied: %v", err)
			}
		})
	}

	if err := MemoryAuthorizer(
		principal,
		mpv1.MemoryService_SearchMemory_FullMethodName,
		&mpv1.SearchMemoryRequest{OrgId: "org-b"},
	); err == nil {
		t.Fatal("session-core token must remain pinned to its signed organization")
	}
}

func TestMemoryAuthorizerPinsTenantForListAndDeleteMemory(t *testing.T) {
	user := authctx.Principal{
		OrganizationID:         "org-a",
		ActorID:                "user-a",
		PrincipalType:          "user",
		RetentionPolicyPresent: true,
	}
	if err := MemoryAuthorizer(user, mpv1.MemoryService_ListMemory_FullMethodName,
		&mpv1.ListMemoryRequest{OrgId: "org-a", UserId: "user-a"}); err != nil {
		t.Fatalf("own tenant list denied: %v", err)
	}
	if err := MemoryAuthorizer(user, mpv1.MemoryService_ListMemory_FullMethodName,
		&mpv1.ListMemoryRequest{OrgId: "org-b", UserId: "user-a"}); err == nil {
		t.Fatal("cross-tenant list must be denied")
	}
	if err := MemoryAuthorizer(user, mpv1.MemoryService_DeleteMemory_FullMethodName,
		&mpv1.DeleteMemoryRequest{OrgId: "org-a", UserId: "user-a", MemoryId: "m1"}); err != nil {
		t.Fatalf("own tenant delete denied: %v", err)
	}
	if err := MemoryAuthorizer(user, mpv1.MemoryService_DeleteMemory_FullMethodName,
		&mpv1.DeleteMemoryRequest{OrgId: "org-b", UserId: "user-a", MemoryId: "m1"}); err == nil {
		t.Fatal("cross-tenant delete must be denied")
	}

	readOnlyService := authctx.Principal{
		OrganizationID:         "org-a",
		ActorID:                "service:model-gateway",
		PrincipalType:          "service",
		Scopes:                 []string{"memory:read"},
		RetentionPolicyPresent: true,
	}
	if err := MemoryAuthorizer(readOnlyService, mpv1.MemoryService_ListMemory_FullMethodName,
		&mpv1.ListMemoryRequest{OrgId: "org-a", UserId: "user-a"}); err != nil {
		t.Fatalf("scoped service list denied: %v", err)
	}
	if err := MemoryAuthorizer(readOnlyService, mpv1.MemoryService_DeleteMemory_FullMethodName,
		&mpv1.DeleteMemoryRequest{OrgId: "org-a", UserId: "user-a", MemoryId: "m1"}); err == nil {
		t.Fatal("read-only service must not delete memory")
	}
}

func TestMemoryAuthorizerRejectsZDRAndUnspecifiedRetentionBeforePersistence(t *testing.T) {
	request := &mpv1.IndexMemoryRequest{OrgId: "org-a"}
	base := authctx.Principal{
		OrganizationID: "org-a",
		ActorID:        "service:session-core",
		PrincipalType:  "service",
		Scopes:         []string{"memory:write"},
	}
	if err := MemoryAuthorizer(base, mpv1.MemoryService_IndexMemory_FullMethodName, request); err == nil {
		t.Fatal("missing signed retention policy must fail closed")
	}
	base.RetentionPolicyPresent = true
	base.ZeroDataRetention = true
	if err := MemoryAuthorizer(base, mpv1.MemoryService_IndexMemory_FullMethodName, request); err == nil {
		t.Fatal("ZDR credential must never reach durable memory")
	}
}
