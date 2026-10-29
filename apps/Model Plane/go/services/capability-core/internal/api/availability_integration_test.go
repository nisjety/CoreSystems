//go:build integration

package api

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// setupAvailabilityDB creates the smallest release-shaped registry needed by
// the signed HTTP availability path. Keeping this as a build-tagged test means
// the ordinary unit suite remains fast while the authorization claim can be
// proven against the real SQL write boundary when Docker/Postgres is present.
func setupAvailabilityDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("capabilities"),
		tcpostgres.WithUsername("test"),
		tcpostgres.WithPassword("test"),
		testcontainers.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").
				WithOccurrence(2).WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres: %v", err)
	}
	t.Cleanup(func() {
		cleanupCtx, cleanupCancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cleanupCancel()
		_ = container.Terminate(cleanupCtx)
	})

	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("connection string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect postgres: %v", err)
	}
	t.Cleanup(pool.Close)

	for _, migration := range []string{
		"0001_models.up.sql",
		"0003_capabilities_registry.up.sql",
		"0006_capability_availability_contract.up.sql",
		"0007_tenant_scopes_and_risk_constraints.up.sql",
	} {
		migrationSQL, readErr := os.ReadFile(filepath.Join("..", "..", "migrations", migration))
		if readErr != nil {
			t.Fatalf("read migration %s: %v", migration, readErr)
		}
		if _, execErr := pool.Exec(ctx, string(migrationSQL)); execErr != nil {
			t.Fatalf("apply migration %s: %v", migration, execErr)
		}
	}
	return pool
}

func TestSignedTenantHealthCannotWriteGlobalCapabilityRowAgainstPostgres(t *testing.T) {
	pool := setupAvailabilityDB(t)
	ctx := context.Background()
	if _, err := pool.Exec(ctx, `
		INSERT INTO capabilities
			(id, org_id, kind, name, version, risk_level, scope, enabled,
			 availability_state, availability_reason_code, execution_mode, cost_class)
		VALUES
			('cap.command.sandbox', 'global', 'tool', 'Sandbox', '1.0.0', 'low', 'org', TRUE,
			 'unavailable', 'health_not_attested', 'unavailable', 'unknown')
	`); err != nil {
		t.Fatalf("seed global capability: %v", err)
	}

	var beforeState string
	var beforeHealthAt *time.Time
	if err := pool.QueryRow(ctx, `
		SELECT availability_state, health_checked_at
		FROM capabilities WHERE id = 'cap.command.sandbox' AND org_id = 'global'
	`).Scan(&beforeState, &beforeHealthAt); err != nil {
		t.Fatalf("read initial capability: %v", err)
	}

	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate test key: %v", err)
	}
	publicKeyPEM := pem.EncodeToMemory(&pem.Block{
		Type: "RSA PUBLIC KEY", Bytes: x509.MarshalPKCS1PublicKey(&privateKey.PublicKey),
	})
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences:    []string{"capability-core"},
		Issuer:       "integration-test-issuer",
		PublicKeyPEM: publicKeyPEM,
	})
	if err != nil {
		t.Fatalf("create verifier: %v", err)
	}
	now := time.Now().UTC()
	token, err := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		// This is a real tenant identity. The request must reach the
		// tenant lookup and still refuse to mutate the global row; using
		// org_id=global here would only prove the earlier Auth Core scope
		// guard, not the owner-row boundary in this handler.
		"org_id":         "tenant-a",
		"service_id":     "tenant-health",
		"principal_type": "service",
		"scopes":         []string{authz.HealthWriteScope},
		"zdr":            false,
		"iss":            "integration-test-issuer",
		"sub":            "tenant-health",
		"aud":            []string{"capability-core"},
		"iat":            now.Unix(),
		"nbf":            now.Unix(),
		"exp":            now.Add(time.Minute).Unix(),
	}).SignedString(privateKey)
	if err != nil {
		t.Fatalf("sign tenant token: %v", err)
	}

	store, err := registry.NewCapabilitiesStore(pool)
	if err != nil {
		t.Fatalf("create capability store: %v", err)
	}
	mux := http.NewServeMux()
	NewCapabilitiesHandler(store).Register(mux)
	secured := verifier.HTTPMiddleware(authz.AuthorizeHTTP)(mux)
	body, _ := json.Marshal(map[string]any{
		"id": "cap.command.sandbox", "version": "1.0.0", "state": "available",
		"reason_code": "runtime_healthy", "reason": "probe succeeded",
		"execution_mode": "direct_read", "cost_class": "bounded",
	})
	request := httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/availability", bytes.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+token)
	recorder := httptest.NewRecorder()
	secured.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusNotFound {
		t.Fatalf("tenant global spoof status = %d, body = %s", recorder.Code, recorder.Body.String())
	}

	var afterState string
	var afterHealthAt *time.Time
	if err := pool.QueryRow(ctx, `
		SELECT availability_state, health_checked_at
		FROM capabilities WHERE id = 'cap.command.sandbox' AND org_id = 'global'
	`).Scan(&afterState, &afterHealthAt); err != nil {
		t.Fatalf("read final capability: %v", err)
	}
	if afterState != beforeState || !sameTime(beforeHealthAt, afterHealthAt) {
		t.Fatalf("global capability changed after denied tenant request: before=(%q,%v) after=(%q,%v)", beforeState, beforeHealthAt, afterState, afterHealthAt)
	}

	var auditCount int
	if err := pool.QueryRow(ctx, `
		SELECT count(*) FROM registry_audit_log
		WHERE entity_kind = 'capability' AND entity_id = 'cap.command.sandbox'
		  AND action = 'global_availability_attested'
	`).Scan(&auditCount); err != nil {
		t.Fatalf("read audit log: %v", err)
	}
	if auditCount != 0 {
		t.Fatalf("denied tenant request created %d global attestation audit rows", auditCount)
	}
}

func TestGenericAndTenantHealthCannotWriteGlobalOwnerActionCapabilityAgainstPostgres(t *testing.T) {
	pool := setupAvailabilityDB(t)
	ctx := context.Background()
	if _, err := pool.Exec(ctx, `
		INSERT INTO capabilities
			(id, org_id, kind, name, version, risk_level, scope, enabled,
			 availability_state, availability_reason_code, execution_mode, cost_class)
		VALUES
			('cap.tool.ticket.create', 'global', 'tool', 'Ticket create', '1.0.0', 'high', 'global', TRUE,
			 'unavailable', 'health_not_attested', 'unavailable', 'unknown')
	`); err != nil {
		t.Fatalf("seed global owner-action capability: %v", err)
	}

	var beforeState string
	var beforeHealthAt *time.Time
	if err := pool.QueryRow(ctx, `
		SELECT availability_state, health_checked_at
		FROM capabilities WHERE id = 'cap.tool.ticket.create' AND org_id = 'global'
	`).Scan(&beforeState, &beforeHealthAt); err != nil {
		t.Fatalf("read initial owner-action capability: %v", err)
	}

	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate test key: %v", err)
	}
	publicKeyPEM := pem.EncodeToMemory(&pem.Block{
		Type: "RSA PUBLIC KEY", Bytes: x509.MarshalPKCS1PublicKey(&privateKey.PublicKey),
	})
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences:    []string{"capability-core"},
		Issuer:       "integration-test-issuer",
		PublicKeyPEM: publicKeyPEM,
	})
	if err != nil {
		t.Fatalf("create verifier: %v", err)
	}
	store, err := registry.NewCapabilitiesStore(pool)
	if err != nil {
		t.Fatalf("create capability store: %v", err)
	}
	mux := http.NewServeMux()
	NewCapabilitiesHandler(store).Register(mux)
	secured := verifier.HTTPMiddleware(authz.AuthorizeHTTP)(mux)

	for _, tc := range []struct {
		name      string
		orgID     string
		serviceID string
		scope     string
	}{
		{name: "tenant health real tenant", orgID: "tenant-a", serviceID: "tenant-health", scope: authz.HealthWriteScope},
		{name: "tenant health forged global", orgID: "global", serviceID: "tenant-health", scope: authz.HealthWriteScope},
		{name: "generic global health", orgID: "global", serviceID: authz.ExecutionCoreServiceID, scope: authz.GlobalHealthWriteScope},
	} {
		t.Run(tc.name, func(t *testing.T) {
			now := time.Now().UTC()
			token, signErr := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
				"org_id":         tc.orgID,
				"service_id":     tc.serviceID,
				"principal_type": "service",
				"scopes":         []string{tc.scope},
				"zdr":            false,
				"iss":            "integration-test-issuer",
				"sub":            tc.serviceID,
				"aud":            []string{"capability-core"},
				"iat":            now.Unix(),
				"nbf":            now.Unix(),
				"exp":            now.Add(time.Minute).Unix(),
			}).SignedString(privateKey)
			if signErr != nil {
				t.Fatalf("sign token: %v", signErr)
			}
			body, marshalErr := json.Marshal(map[string]any{
				"id": "cap.tool.ticket.create", "version": "1.0.0", "state": "available",
				"reason_code": "runtime_healthy", "reason": "probe succeeded",
				"execution_mode": "agentic", "cost_class": "bounded",
			})
			if marshalErr != nil {
				t.Fatalf("marshal request: %v", marshalErr)
			}
			request := httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/availability", bytes.NewReader(body))
			request.Header.Set("Authorization", "Bearer "+token)
			recorder := httptest.NewRecorder()
			secured.ServeHTTP(recorder, request)
			if recorder.Code != http.StatusForbidden {
				t.Fatalf("%s status = %d, body = %s", tc.name, recorder.Code, recorder.Body.String())
			}
		})
	}

	var afterState string
	var afterHealthAt *time.Time
	if err := pool.QueryRow(ctx, `
		SELECT availability_state, health_checked_at
		FROM capabilities WHERE id = 'cap.tool.ticket.create' AND org_id = 'global'
	`).Scan(&afterState, &afterHealthAt); err != nil {
		t.Fatalf("read final owner-action capability: %v", err)
	}
	if afterState != beforeState || !sameTime(beforeHealthAt, afterHealthAt) {
		t.Fatalf("global owner-action capability changed after denied requests: before=(%q,%v) after=(%q,%v)", beforeState, beforeHealthAt, afterState, afterHealthAt)
	}

	var auditCount int
	if err := pool.QueryRow(ctx, `
		SELECT count(*) FROM registry_audit_log
		WHERE entity_kind = 'capability' AND entity_id = 'cap.tool.ticket.create'
		  AND action = 'global_availability_attested'
	`).Scan(&auditCount); err != nil {
		t.Fatalf("read owner-action audit log: %v", err)
	}
	if auditCount != 0 {
		t.Fatalf("denied health requests created %d global owner-action attestation audit rows", auditCount)
	}
}

func sameTime(left, right *time.Time) bool {
	if left == nil || right == nil {
		return left == nil && right == nil
	}
	return left.Equal(*right)
}
