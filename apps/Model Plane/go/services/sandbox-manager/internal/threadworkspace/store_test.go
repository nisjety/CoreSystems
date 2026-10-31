package threadworkspace

import (
	"errors"
	"testing"
)

var testLimits = Limits{MaxFiles: 4, MaxBytes: 100}

func file(p, hash string, size int64) File {
	return File{Path: p, ContentHash: hash, SizeBytes: size}
}

func currentOf(files ...File) map[string]File {
	m := make(map[string]File, len(files))
	for _, f := range files {
		m[f.Path] = f
	}
	return m
}

func TestPlanChangesCreatesAPathThatDoesNotExist(t *testing.T) {
	plan, err := PlanChanges(currentOf(), []Change{{Path: "notes.txt", ContentHash: "sha256:a", SizeBytes: 5}}, nil, testLimits)
	if err != nil {
		t.Fatalf("PlanChanges: %v", err)
	}
	if len(plan.Upserts) != 1 || plan.Upserts[0].Path != "notes.txt" || len(plan.Conflicts) != 0 {
		t.Fatalf("plan = %+v, want one upsert and no conflict", plan)
	}
	if plan.FileCount != 1 || plan.TotalBytes != 5 {
		t.Fatalf("counts = %d files %d bytes, want 1 and 5", plan.FileCount, plan.TotalBytes)
	}
}

func TestPlanChangesAppliesWhenTheBaseHashStillMatches(t *testing.T) {
	cur := currentOf(file("a.txt", "sha256:old", 3))
	plan, err := PlanChanges(cur, []Change{{Path: "a.txt", ContentHash: "sha256:new", SizeBytes: 4, BaseHash: "sha256:old"}}, nil, testLimits)
	if err != nil {
		t.Fatalf("PlanChanges: %v", err)
	}
	if len(plan.Upserts) != 1 || len(plan.Conflicts) != 0 || plan.TotalBytes != 4 {
		t.Fatalf("plan = %+v, want the rewrite applied", plan)
	}
}

func TestPlanChangesReportsAStaleBaseWithoutOverwriting(t *testing.T) {
	cur := currentOf(file("a.txt", "sha256:winner", 3))
	plan, err := PlanChanges(cur, []Change{{Path: "a.txt", ContentHash: "sha256:loser", SizeBytes: 9, BaseHash: "sha256:original"}}, nil, testLimits)
	if err != nil {
		t.Fatalf("PlanChanges: %v", err)
	}
	if len(plan.Conflicts) != 1 || plan.Conflicts[0] != "a.txt" {
		t.Fatalf("conflicts = %v, want [a.txt]", plan.Conflicts)
	}
	if len(plan.Upserts) != 0 || plan.Changed() {
		t.Fatalf("plan = %+v, want the conflicting write dropped, not applied", plan)
	}
}

func TestPlanChangesRefusesAnAbsentBaseWhenThePathExists(t *testing.T) {
	cur := currentOf(file("a.txt", "sha256:there", 3))
	plan, err := PlanChanges(cur, []Change{{Path: "a.txt", ContentHash: "sha256:mine", SizeBytes: 2}}, nil, testLimits)
	if err != nil {
		t.Fatalf("PlanChanges: %v", err)
	}
	if len(plan.Conflicts) != 1 {
		t.Fatalf("conflicts = %v, want the create to conflict with a path that exists", plan.Conflicts)
	}
}

func TestPlanChangesTreatsARetryOfAnAppliedWriteAsANoOp(t *testing.T) {
	cur := currentOf(file("a.txt", "sha256:new", 4))
	plan, err := PlanChanges(cur, []Change{{Path: "a.txt", ContentHash: "sha256:new", SizeBytes: 4, BaseHash: "sha256:old"}}, nil, testLimits)
	if err != nil {
		t.Fatalf("PlanChanges: %v", err)
	}
	if len(plan.Conflicts) != 0 || plan.Changed() {
		t.Fatalf("plan = %+v, want a retry to be a no-op and not a false conflict", plan)
	}
}

func TestPlanChangesAppliesNonConflictingPathsIndependently(t *testing.T) {
	cur := currentOf(file("taken.txt", "sha256:winner", 1))
	plan, err := PlanChanges(cur, []Change{
		{Path: "taken.txt", ContentHash: "sha256:loser", SizeBytes: 1, BaseHash: "sha256:stale"},
		{Path: "free.txt", ContentHash: "sha256:free", SizeBytes: 2},
	}, nil, testLimits)
	if err != nil {
		t.Fatalf("PlanChanges: %v", err)
	}
	if len(plan.Conflicts) != 1 || plan.Conflicts[0] != "taken.txt" {
		t.Fatalf("conflicts = %v, want [taken.txt]", plan.Conflicts)
	}
	if len(plan.Upserts) != 1 || plan.Upserts[0].Path != "free.txt" {
		t.Fatalf("upserts = %+v, want free.txt to apply", plan.Upserts)
	}
}

func TestPlanChangesDeletesOnAMatchingBaseAndReportsAMismatch(t *testing.T) {
	cur := currentOf(file("a.txt", "sha256:one", 1), file("b.txt", "sha256:two", 1))
	plan, err := PlanChanges(cur, nil, []Deletion{
		{Path: "a.txt", BaseHash: "sha256:one"},
		{Path: "b.txt", BaseHash: "sha256:other"},
	}, testLimits)
	if err != nil {
		t.Fatalf("PlanChanges: %v", err)
	}
	if len(plan.Deletes) != 1 || plan.Deletes[0] != "a.txt" {
		t.Fatalf("deletes = %v, want [a.txt]", plan.Deletes)
	}
	if len(plan.Conflicts) != 1 || plan.Conflicts[0] != "b.txt" {
		t.Fatalf("conflicts = %v, want [b.txt]", plan.Conflicts)
	}
	if plan.FileCount != 1 {
		t.Fatalf("file count = %d, want 1 after the delete", plan.FileCount)
	}
}

func TestPlanChangesDeleteOfAnAlreadyGonePathIsANoOp(t *testing.T) {
	plan, err := PlanChanges(currentOf(), nil, []Deletion{{Path: "gone.txt", BaseHash: "sha256:x"}}, testLimits)
	if err != nil {
		t.Fatalf("PlanChanges: %v", err)
	}
	if plan.Changed() || len(plan.Conflicts) != 0 {
		t.Fatalf("plan = %+v, want a repeated delete to change nothing", plan)
	}
}

func TestPlanChangesRefusesAWholeBatchOverTheFileCap(t *testing.T) {
	cur := currentOf(file("a", "sha256:a", 1), file("b", "sha256:b", 1), file("c", "sha256:c", 1), file("d", "sha256:d", 1))
	_, err := PlanChanges(cur, []Change{{Path: "e", ContentHash: "sha256:e", SizeBytes: 1}}, nil, testLimits)
	if !errors.Is(err, ErrQuotaExceeded) {
		t.Fatalf("err = %v, want ErrQuotaExceeded for a fifth file under a four-file cap", err)
	}
}

func TestPlanChangesRefusesAWholeBatchOverTheByteCap(t *testing.T) {
	_, err := PlanChanges(currentOf(), []Change{{Path: "big", ContentHash: "sha256:big", SizeBytes: 101}}, nil, testLimits)
	if !errors.Is(err, ErrQuotaExceeded) {
		t.Fatalf("err = %v, want ErrQuotaExceeded for a file over the byte cap", err)
	}
}

func TestPlanChangesCountsTheStateAfterTheBatchNotTheSum(t *testing.T) {
	// Replacing a file of 90 bytes with one of 95 stays inside a 100-byte cap,
	// because the state after the batch is 95 bytes, not 185.
	cur := currentOf(file("a.txt", "sha256:old", 90))
	_, err := PlanChanges(cur, []Change{{Path: "a.txt", ContentHash: "sha256:new", SizeBytes: 95, BaseHash: "sha256:old"}}, nil, testLimits)
	if err != nil {
		t.Fatalf("PlanChanges: %v, want a rewrite that keeps the total under the cap to apply", err)
	}
}

func TestPlanChangesRefusesPathsThatEscapeOrRepeat(t *testing.T) {
	for _, bad := range []string{"", "/etc/passwd", "../up.txt", "a/../b", "a//b", ".."} {
		_, err := PlanChanges(currentOf(), []Change{{Path: bad, ContentHash: "sha256:x"}}, nil, testLimits)
		if !errors.Is(err, ErrInvalidChange) {
			t.Fatalf("path %q: err = %v, want ErrInvalidChange", bad, err)
		}
	}
	_, err := PlanChanges(currentOf(), []Change{{Path: "a.txt", ContentHash: "sha256:x"}}, []Deletion{{Path: "a.txt"}}, testLimits)
	if !errors.Is(err, ErrInvalidChange) {
		t.Fatalf("a path both changed and deleted: err = %v, want ErrInvalidChange", err)
	}
}

func TestPlanChangesRequiresAContentHash(t *testing.T) {
	_, err := PlanChanges(currentOf(), []Change{{Path: "a.txt"}}, nil, testLimits)
	if !errors.Is(err, ErrInvalidChange) {
		t.Fatalf("err = %v, want ErrInvalidChange for a change with no content_hash", err)
	}
}

func TestLimitsFromEnvUsesTheDefaultsWhenUnset(t *testing.T) {
	limits, err := LimitsFromEnv(func(string) string { return "" })
	if err != nil {
		t.Fatalf("LimitsFromEnv: %v", err)
	}
	if limits.MaxFiles != DefaultMaxFiles || limits.MaxBytes != DefaultMaxBytes {
		t.Fatalf("limits = %+v, want the DEC-23 defaults", limits)
	}
}

func TestLimitsFromEnvRefusesAMalformedCap(t *testing.T) {
	env := map[string]string{"SANDBOX_MANAGER_THREAD_MAX_FILES": "lots"}
	if _, err := LimitsFromEnv(func(k string) string { return env[k] }); err == nil {
		t.Fatal("LimitsFromEnv accepted a non-numeric file cap")
	}
	env = map[string]string{"SANDBOX_MANAGER_THREAD_MAX_BYTES": "0"}
	if _, err := LimitsFromEnv(func(k string) string { return env[k] }); err == nil {
		t.Fatal("LimitsFromEnv accepted a zero byte cap")
	}
}
