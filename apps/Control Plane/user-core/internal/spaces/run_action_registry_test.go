package spaces

import (
	"encoding/json"
	"slices"
	"strings"
	"testing"
	"time"
)

// WS18-S1: the vendored Space self-API registry copy and the run-action lookups
// Control derives from it (runActionSpec, the operator allowlist and each row's
// own permission and role floor).

// runActionPersonalEvidence is the personal Space owner's agent-action evidence the
// run-action tests share; validRunActionAuthority is its run binding.
func runActionPersonalEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	return evidence
}

// goldenRunActionRequest is validRunActionRequest for any golden row, carrying the
// row's own schema hash.
func goldenRunActionRequest(t *testing.T, actionID string) RunActionDecisionRequest {
	t.Helper()
	spec, found := runActionSpec(actionID)
	if !found {
		t.Fatalf("the golden registry has no %s row", actionID)
	}
	request := validRunActionRequest()
	request.ActionID = spec.ActionID
	request.ActionSchemaHash = spec.SchemaSHA256
	request.IdempotencyKey = "run-1:" + spec.ActionID + ":1"
	return request
}

// withRunActionRegistryRow changes one golden row for a test and restores the
// registry after it. Decisions and views read the changed row through runActionSpec.
func withRunActionRegistryRow(t *testing.T, actionID string, mutate func(*runActionSpecRow)) {
	t.Helper()
	previous := runActionRegistry
	rows := make(map[string]runActionSpecRow, len(previous))
	for id, row := range previous {
		rows[id] = row
	}
	row, found := rows[actionID]
	if !found {
		t.Fatalf("the golden registry has no %s row", actionID)
	}
	mutate(&row)
	rows[actionID] = row
	runActionRegistry = rows
	t.Cleanup(func() { runActionRegistry = previous })
}

// The embedded copy loads as one strict document with the golden's nine rows, and
// its tickets.create row carries exactly the values Control pinned for that action
// before the registry existed.
func TestRunActionRegistryMatchesGolden(t *testing.T) {
	if runActionRegistryErr != nil {
		t.Fatalf("the embedded registry copy did not load: %v", runActionRegistryErr)
	}
	rows, err := loadRunActionRegistry(spaceSelfAPIRegistryJSON)
	if err != nil {
		t.Fatalf("loadRunActionRegistry: %v", err)
	}
	wantIDs := []string{
		"space.agent.pause_self", "space.canvas.read", "space.canvas.write", "space.directory.resolve",
		"space.schedule.follow_up", "space.thread.pin", "space.thread.post_card", "space.thread.rename", "tickets.create",
	}
	if got := runActionRegistryIDs(); !slices.Equal(got, wantIDs) || len(rows) != len(wantIDs) {
		t.Fatalf("registry ids = %v (%d rows), want %v", got, len(rows), wantIDs)
	}

	tickets := rows[ticketsCreateActionID]
	if tickets.ServiceAudience != ticketsCreateServiceAudience || tickets.Permission != ticketsCreateActionPermission ||
		tickets.SchemaSHA256 != ticketsCreateActionSchemaSHA256 || tickets.HashBasis != runActionHashBasisOwnerContract ||
		tickets.DecisionType != runActionDecisionKind || tickets.MinRole != RoleEditor ||
		tickets.Approval != "owner_approval" || !tickets.Reservation {
		t.Fatalf("tickets.create row = %+v", tickets)
	}
	target, found := runActionTargetFor(ticketsCreateActionID)
	if !found || target.ServiceAudience != tickets.ServiceAudience || target.Permission != tickets.Permission ||
		target.Class != runActionClassWriteApproval || target.HashBasis != tickets.HashBasis || target.SchemaHash != tickets.SchemaSHA256 {
		t.Fatalf("tickets.create target row = %+v", target)
	}

	// Every Space row pins the SHA-256 of its exact parameter bytes and reserves no
	// owner effect (migration 025 admits tickets.create only).
	for id, row := range rows {
		if id == ticketsCreateActionID {
			continue
		}
		if row.HashBasis != "parameters_json" || row.SchemaSHA256 != runActionSchemaCommitment(row.ParametersJSON) || row.Reservation {
			t.Errorf("row %s: basis %q, hash %q, reservation %v", id, row.HashBasis, row.SchemaSHA256, row.Reservation)
		}
	}
}

// A Space row issues only while CONTROL_SPACE_SELF_API_ACTIONS names it exactly;
// tickets.create predates the list and is exempt. The effect-time check repeats the
// list, so removing an entry stops a decision already issued.
func TestRunActionRefusedWhenNotInAllowedSet(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := runActionPersonalEvidence()
	evidence.CanvasEntitled = true
	spaceRows := []string{
		"space.agent.pause_self", "space.canvas.read", "space.canvas.write",
		"space.thread.pin", "space.thread.post_card", "space.thread.rename",
	}
	issue := func(actionID string) (RunActionDecision, error) {
		return IssueRunActionDecision(evidence, validRunActionAuthority(), goldenRunActionRequest(t, actionID), now)
	}

	t.Setenv(spaceSelfAPIActionsEnv, "")
	for _, id := range spaceRows {
		if _, err := issue(id); err == nil {
			t.Errorf("%s issued with no allowlist", id)
		}
	}
	if _, err := issue(ticketsCreateActionID); err != nil {
		t.Fatalf("tickets.create with no allowlist: %v", err)
	}

	// Each entry is trimmed and must match exactly: a case variant, a prefix or a
	// longer id names nothing.
	t.Setenv(spaceSelfAPIActionsEnv, " space.thread.rename ,SPACE.THREAD.PIN,space.thread.post,space.agent.pause_self.extra")
	if _, err := issue("space.thread.rename"); err != nil {
		t.Fatalf("an allowlisted row was refused: %v", err)
	}
	for _, id := range []string{"space.thread.pin", "space.thread.post_card", "space.agent.pause_self"} {
		if _, err := issue(id); err == nil {
			t.Errorf("%s issued without an exact allowlist entry", id)
		}
	}

	t.Setenv(spaceSelfAPIActionsEnv, strings.Join(spaceRows, ","))
	for _, id := range spaceRows {
		if _, err := issue(id); err != nil {
			t.Errorf("%s was refused although allowlisted: %v", id, err)
		}
	}

	// The schedule row is a schedule_create decision; naming it never makes it a run action.
	t.Setenv(spaceSelfAPIActionsEnv, "space.schedule.follow_up")
	if _, err := issue("space.schedule.follow_up"); err == nil {
		t.Fatal("the schedule row was issued as a run action")
	}

	t.Setenv(spaceSelfAPIActionsEnv, "space.thread.rename")
	rename, err := issue("space.thread.rename")
	if err != nil {
		t.Fatalf("issue space.thread.rename: %v", err)
	}
	tickets, err := issue(ticketsCreateActionID)
	if err != nil {
		t.Fatalf("issue tickets.create: %v", err)
	}
	if err := ValidateCurrentRunActionDecision(evidence, rename); err != nil {
		t.Fatalf("an allowlisted decision failed the effect-time check: %v", err)
	}
	t.Setenv(spaceSelfAPIActionsEnv, "")
	if err := ValidateCurrentRunActionDecision(evidence, rename); err == nil {
		t.Fatal("a decision whose allowlist entry was removed passed the effect-time check")
	}
	if err := ValidateCurrentRunActionDecision(evidence, tickets); err != nil {
		t.Fatalf("tickets.create failed the effect-time check with no allowlist: %v", err)
	}
}

// Each row decides under its own audience and permission: the Space self-API rows
// under space-action:execute, the legacy owner row under owner-action:execute. A
// decision carrying the other lane's permission does not validate.
func TestRunActionPermissionPerRow(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := runActionPersonalEvidence()
	t.Setenv(spaceSelfAPIActionsEnv, "space.agent.pause_self,space.thread.pin,space.thread.post_card,space.thread.rename")
	for id, want := range map[string]struct{ audience, permission, other string }{
		"tickets.create":         {"application-plane-conversation-core", "owner-action:execute", "space-action:execute"},
		"space.thread.rename":    {"model-plane-session-core", "space-action:execute", "owner-action:execute"},
		"space.thread.pin":       {"model-plane-session-core", "space-action:execute", "owner-action:execute"},
		"space.thread.post_card": {"model-plane-session-core", "space-action:execute", "owner-action:execute"},
		"space.agent.pause_self": {"application-plane-convex-core", "space-action:execute", "owner-action:execute"},
	} {
		t.Run(id, func(t *testing.T) {
			decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), goldenRunActionRequest(t, id), now)
			if err != nil {
				t.Fatalf("IssueRunActionDecision: %v", err)
			}
			if decision.ServiceAudience != want.audience || len(decision.Permissions) != 1 || decision.Permissions[0] != want.permission {
				t.Fatalf("decision audience %q permissions %v, want %q [%s]", decision.ServiceAudience, decision.Permissions, want.audience, want.permission)
			}
			if err := decision.Validate(); err != nil {
				t.Fatalf("issued decision did not validate: %v", err)
			}
			swapped := decision
			swapped.Permissions = []string{want.other}
			if err := swapped.Validate(); err == nil {
				t.Fatalf("%s validated under %s", id, want.other)
			}
		})
	}
}

// A copy the loader cannot check alone yields no rows at all, and with no rows every
// lookup, decision and view fails closed.
func TestRunActionRefusesMalformedRegistry(t *testing.T) {
	var golden runActionRegistryDocument
	if err := json.Unmarshal(spaceSelfAPIRegistryJSON, &golden); err != nil {
		t.Fatalf("decode the embedded registry: %v", err)
	}
	rename := slices.IndexFunc(golden.Rows, func(row runActionSpecRow) bool { return row.ActionID == "space.thread.rename" })
	if rename < 0 {
		t.Fatal("the golden registry has no space.thread.rename row")
	}
	encode := func(t *testing.T, mutate func(*runActionRegistryDocument)) []byte {
		t.Helper()
		doc := golden
		doc.Rows = append([]runActionSpecRow(nil), golden.Rows...)
		mutate(&doc)
		raw, err := json.Marshal(doc)
		if err != nil {
			t.Fatalf("encode registry: %v", err)
		}
		return raw
	}
	// withParameters gives the rename row new parameters and their true hash, so the
	// case fails on the parameters alone.
	withParameters := func(doc *runActionRegistryDocument, argument, parameters string) {
		row := &doc.Rows[rename]
		row.ArgumentOrder = []string{argument}
		row.ParametersJSON = parameters
		row.SchemaSHA256 = runActionSchemaCommitment(parameters)
	}
	if rows, err := loadRunActionRegistry(encode(t, func(*runActionRegistryDocument) {})); err != nil || len(rows) != len(golden.Rows) {
		t.Fatalf("the re-encoded golden must still load: %d rows, %v", len(rows), err)
	}

	// Each case names the refusal it must hit, so it cannot pass on another fault.
	for name, test := range map[string]struct {
		mutate func(*runActionRegistryDocument)
		want   string
	}{
		"forbidden action name": {func(d *runActionRegistryDocument) { d.Rows[rename].ActionID = "space.member.remove" }, "names a forbidden part"},
		"forbidden argument name": {func(d *runActionRegistryDocument) {
			withParameters(d, "user_id", `{"type":"object","additionalProperties":false,"properties":{"user_id":{"type":"string"}},"required":["user_id"]}`)
		}, "argument is invalid"},
		"reservation on a Space row":          {func(d *runActionRegistryDocument) { d.Rows[rename].Reservation = true }, "may not reserve an owner effect"},
		"owner contract basis on a Space row": {func(d *runActionRegistryDocument) { d.Rows[rename].HashBasis = "owner_contract" }, "may not pin an owner contract"},
		"unknown hash basis":                  {func(d *runActionRegistryDocument) { d.Rows[rename].HashBasis = "catalog_entry" }, "unknown hash basis"},
		"hash not of the parameter bytes": {func(d *runActionRegistryDocument) {
			d.Rows[rename].SchemaSHA256 = "sha256:" + strings.Repeat("a", 64)
		}, "does not match its parameters"},
		"unsupported property type": {func(d *runActionRegistryDocument) {
			withParameters(d, "title", `{"type":"object","additionalProperties":false,"properties":{"title":{"type":"object"}},"required":["title"]}`)
		}, "unsupported type"},
		"format keyword": {func(d *runActionRegistryDocument) {
			withParameters(d, "title", `{"type":"object","additionalProperties":false,"properties":{"title":{"type":"string","format":"uri"}},"required":["title"]}`)
		}, "not a closed object"},
		"open parameter object": {func(d *runActionRegistryDocument) {
			withParameters(d, "title", `{"type":"object","additionalProperties":true,"properties":{"title":{"type":"string"}},"required":["title"]}`)
		}, "not a closed object"},
		"repeated action id": {func(d *runActionRegistryDocument) { d.Rows = append(d.Rows, d.Rows[rename]) }, "repeats an action id"},
		"wrong contract":     {func(d *runActionRegistryDocument) { d.Contract = "space-self-api-registry-v0" }, "header is invalid"},
	} {
		t.Run(name, func(t *testing.T) {
			rows, err := loadRunActionRegistry(encode(t, test.mutate))
			if err == nil || rows != nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("loaded %d rows, err %v; want the refusal %q", len(rows), err, test.want)
			}
		})
	}

	// The document itself must be one strict object.
	withField := strings.Replace(string(spaceSelfAPIRegistryJSON), `"phase": 0`, `"phase": 0, "extra": true`, 1)
	if withField == string(spaceSelfAPIRegistryJSON) {
		t.Fatal("the golden has no tickets.create phase field to extend")
	}
	for name, raw := range map[string]string{
		"unknown row field": withField,
		"trailing document": string(spaceSelfAPIRegistryJSON) + "{}",
	} {
		rows, err := loadRunActionRegistry([]byte(raw))
		if err == nil || rows != nil || !strings.Contains(err.Error(), "not one strict JSON document") {
			t.Errorf("%s: loaded %d rows, err %v", name, len(rows), err)
		}
	}

	// A malformed embedded copy fails every lookup closed.
	previousRows, previousErr := runActionRegistry, runActionRegistryErr
	t.Cleanup(func() { runActionRegistry, runActionRegistryErr = previousRows, previousErr })
	runActionRegistry, runActionRegistryErr = loadRunActionRegistry(encode(t, func(d *runActionRegistryDocument) { d.Rows[rename].Reservation = true }))
	if runActionRegistryErr == nil {
		t.Fatal("the malformed copy loaded")
	}
	t.Setenv(spaceSelfAPIActionsEnv, "tickets.create,space.thread.rename")
	if _, found := runActionSpec(ticketsCreateActionID); found {
		t.Fatal("a malformed copy still resolved tickets.create")
	}
	if ids := runActionRegistryIDs(); len(ids) != 0 {
		t.Fatalf("a malformed copy listed %v", ids)
	}
	if rows := runActionTargetRowsFromRegistry(); len(rows) != 0 {
		t.Fatalf("a malformed copy projected %d target rows", len(rows))
	}
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := runActionPersonalEvidence()
	if _, err := IssueRunActionDecision(evidence, validRunActionAuthority(), validRunActionRequest(), now); err == nil {
		t.Fatal("a malformed copy issued a tickets.create decision")
	}
	if _, err := IssueModelActionViewV2(evidence, validRunActionAuthority(), ModelActionViewRequest{DecisionRef: "view-1", Nonce: "nonce-1"}, now); err == nil {
		t.Fatal("a malformed copy issued a v2 view")
	}
}
