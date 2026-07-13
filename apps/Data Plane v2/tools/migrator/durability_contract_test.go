package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestQualityAndOrchestratorDurabilityMigrationContract(t *testing.T) {
	path := filepath.Join("..", "..", "infra", "postgres", "migrations", "20260711160000_quality_orchestrator_durability.sql")
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read durability migration: %v", err)
	}
	sql := string(contents)
	for _, required := range []string{
		"CREATE TABLE IF NOT EXISTS quality_eval_runs",
		"CREATE TABLE IF NOT EXISTS data_orchestrator_jobs",
		"quality_eval_runs_status_check",
		"quality_eval_runs_identity_check",
		"data_orchestrator_jobs_status_check",
		"data_orchestrator_jobs_identity_check",
		"data_orchestrator_jobs_documents_shape_check",
		"quality_eval_runs_org_id_idempotency_key_idx",
		"data_orchestrator_jobs_org_id_idempotency_key_idx",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("migration missing %q", required)
		}
	}

	downPath := filepath.Join("..", "..", "infra", "postgres", "migrations", "20260711160000_quality_orchestrator_durability.down.sql")
	down, err := os.ReadFile(downPath)
	if err != nil {
		t.Fatalf("read durability rollback: %v", err)
	}
	for _, required := range []string{
		"DROP TABLE IF EXISTS data_orchestrator_jobs",
		"DROP TABLE IF EXISTS quality_eval_runs",
	} {
		if !strings.Contains(string(down), required) {
			t.Errorf("rollback missing %q", required)
		}
	}
}
