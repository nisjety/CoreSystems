package modelentitlement

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"reflect"
	"sort"
	"strings"
	"testing"
)

const (
	matrixPath = "testdata/model-entitlement-matrix.json"
	// matrixCanonicalPath is the authored copy in the Model Plane contracts
	// folder. It sits outside this module (org-core's Docker build context is
	// the module), so a missing copy skips the byte comparison unless
	// REQUIRE_CONTRACT is set, as the compose-based contract tests do.
	matrixCanonicalPath = "../../../../Model Plane/api/contracts/model-entitlement-matrix.json"
)

type matrixDoc struct {
	Contract    string      `json:"contract"`
	Version     int         `json:"version"`
	Description string      `json:"description"`
	Vocabulary  matrixVocab `json:"vocabulary"`
	Rows        []matrixRow `json:"rows"`
}

type matrixVocab struct {
	Kinds        []string `json:"kinds"`
	Evaluators   []string `json:"evaluators"`
	States       []string `json:"states"`
	Layers       []string `json:"layers"`
	Reasons      []string `json:"reasons"`
	RequiredRows []string `json:"required_rows"`
}

type matrixRow struct {
	ID          string          `json:"id"`
	Kind        string          `json:"kind"`
	EvaluatedBy []string        `json:"evaluated_by"`
	Note        string          `json:"note"`
	Input       json.RawMessage `json:"input"`
	Expect      json.RawMessage `json:"expect"`
}

func (row matrixRow) evaluatedByOrgCore() bool {
	for _, evaluator := range row.EvaluatedBy {
		if evaluator == "org-core" {
			return true
		}
	}
	return false
}

func strictDecode(t *testing.T, raw []byte, into any) {
	t.Helper()
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(into); err != nil {
		t.Fatalf("decode %s: %v", string(raw), err)
	}
}

func loadMatrix(t *testing.T) matrixDoc {
	t.Helper()
	raw, err := os.ReadFile(matrixPath)
	if err != nil {
		t.Fatalf("read vendored matrix %s: %v", matrixPath, err)
	}
	var doc matrixDoc
	strictDecode(t, raw, &doc)
	if doc.Contract != "model-entitlement.matrix" || doc.Version != 1 || len(doc.Rows) == 0 {
		t.Fatalf("matrix is not a version 1 model-entitlement.matrix with rows: %q v%d (%d rows)", doc.Contract, doc.Version, len(doc.Rows))
	}
	return doc
}

func normaliseLineEndings(data []byte) []byte {
	return bytes.ReplaceAll(data, []byte("\r\n"), []byte("\n"))
}

func TestMatrixIsVendoredByteForByte(t *testing.T) {
	vendored, err := os.ReadFile(matrixPath)
	if err != nil {
		t.Fatalf("read vendored matrix: %v", err)
	}
	canonical, err := os.ReadFile(matrixCanonicalPath)
	if err != nil {
		if os.IsNotExist(err) && os.Getenv("REQUIRE_CONTRACT") == "" {
			t.Skipf("%s is absent (set REQUIRE_CONTRACT=1 to make that a failure)", matrixCanonicalPath)
		}
		t.Fatalf("read %s: %v", matrixCanonicalPath, err)
	}
	if !bytes.Equal(normaliseLineEndings(vendored), normaliseLineEndings(canonical)) {
		t.Fatalf("%s differs from %s; copy the authored file over the vendored one, never edit the vendored copy", matrixPath, matrixCanonicalPath)
	}
}

func contains(list []string, value string) bool {
	for _, entry := range list {
		if entry == value {
			return true
		}
	}
	return false
}

func TestMatrixRowsAreWellFormed(t *testing.T) {
	doc := loadMatrix(t)
	seen := map[string]struct{}{}
	for _, row := range doc.Rows {
		if row.ID == "" {
			t.Fatalf("a row has no id: %+v", row)
		}
		if _, duplicate := seen[row.ID]; duplicate {
			t.Fatalf("row id %s appears twice", row.ID)
		}
		seen[row.ID] = struct{}{}
		if !contains(doc.Vocabulary.Kinds, row.Kind) {
			t.Fatalf("row %s has kind %q outside the vocabulary", row.ID, row.Kind)
		}
		if len(row.EvaluatedBy) == 0 || strings.TrimSpace(row.Note) == "" || len(row.Input) == 0 || len(row.Expect) == 0 {
			t.Fatalf("row %s needs evaluated_by, a note, an input and an expect", row.ID)
		}
		for _, evaluator := range row.EvaluatedBy {
			if !contains(doc.Vocabulary.Evaluators, evaluator) {
				t.Fatalf("row %s names evaluator %q outside the vocabulary", row.ID, evaluator)
			}
		}
	}
	for _, required := range doc.Vocabulary.RequiredRows {
		if _, ok := seen[required]; !ok {
			t.Fatalf("required row %s is missing from the matrix", required)
		}
	}
}

// The matrix is a specification, so it must stay free of content: a prompt or a
// message in a row would be a retention-class change nobody reviewed.
func TestMatrixCarriesNoContentKeys(t *testing.T) {
	raw, err := os.ReadFile(matrixPath)
	if err != nil {
		t.Fatalf("read matrix: %v", err)
	}
	var tree any
	if err := json.Unmarshal(raw, &tree); err != nil {
		t.Fatalf("decode matrix: %v", err)
	}
	forbidden := map[string]struct{}{"prompt": {}, "content": {}, "completion": {}, "messages": {}, "input_text": {}, "output_text": {}}
	var walk func(node any)
	walk = func(node any) {
		switch typed := node.(type) {
		case map[string]any:
			for key, child := range typed {
				if _, bad := forbidden[key]; bad {
					t.Fatalf("matrix carries a content key %q", key)
				}
				walk(child)
			}
		case []any:
			for _, child := range typed {
				walk(child)
			}
		}
	}
	walk(tree)
}

type matrixResolveInput struct {
	Set struct {
		Revision   int64             `json:"revision"`
		OrgDefault *Policy           `json:"org_default"`
		Roles      map[string]Policy `json:"roles"`
	} `json:"set"`
	Caller struct {
		UserID string `json:"user_id"`
		Role   string `json:"role"`
	} `json:"caller"`
	Zdr []bool `json:"zdr"`
}

func asTree(t *testing.T, value any) any {
	t.Helper()
	encoded, err := json.Marshal(value)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var tree any
	if err := json.Unmarshal(encoded, &tree); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	return tree
}

func rawTree(t *testing.T, raw []byte) any {
	t.Helper()
	var tree any
	if err := json.Unmarshal(raw, &tree); err != nil {
		t.Fatalf("decode %s: %v", string(raw), err)
	}
	return tree
}

func TestMatrixResolveRows(t *testing.T) {
	doc := loadMatrix(t)
	ran := 0
	for _, row := range doc.Rows {
		if row.Kind != "resolve" || !row.evaluatedByOrgCore() {
			continue
		}
		ran++
		t.Run(row.ID, func(t *testing.T) {
			var input matrixResolveInput
			strictDecode(t, row.Input, &input)
			set := &PolicySet{Revision: input.Set.Revision, OrgDefault: input.Set.OrgDefault, Roles: input.Set.Roles}
			got := asTree(t, Resolve(set, Caller{UserID: input.Caller.UserID, Role: input.Caller.Role}))
			want := rawTree(t, row.Expect)
			if !reflect.DeepEqual(got, want) {
				t.Fatalf("resolution mismatch\n got: %v\nwant: %v", got, want)
			}
			// The resolver takes no retention input, so every zdr variant a row
			// lists must resolve identically (z_policy_survives_zdr_toggle).
			for range input.Zdr {
				again := asTree(t, Resolve(set, Caller{UserID: input.Caller.UserID, Role: input.Caller.Role}))
				if !reflect.DeepEqual(again, want) {
					t.Fatalf("resolution changed between zdr variants: %v", again)
				}
			}
		})
	}
	if ran == 0 {
		t.Fatal("no resolve rows were evaluated")
	}
}

func TestMatrixPermitRows(t *testing.T) {
	doc := loadMatrix(t)
	ran := 0
	for _, row := range doc.Rows {
		if row.Kind != "permit" || !row.evaluatedByOrgCore() {
			continue
		}
		ran++
		t.Run(row.ID, func(t *testing.T) {
			var input struct {
				Policy   Policy `json:"policy"`
				Model    string `json:"model"`
				Provider string `json:"provider"`
			}
			var expect struct {
				Permitted bool   `json:"permitted"`
				Reason    string `json:"reason"`
			}
			strictDecode(t, row.Input, &input)
			strictDecode(t, row.Expect, &expect)
			permitted, reason := input.Policy.Permits(input.Model, input.Provider)
			if permitted != expect.Permitted || reason != expect.Reason {
				t.Fatalf("Permits(%q, %q) = %v %q; want %v %q", input.Model, input.Provider, permitted, reason, expect.Permitted, expect.Reason)
			}
		})
	}
	if ran == 0 {
		t.Fatal("no permit rows were evaluated")
	}
}

// TestMatrixRouteRows evaluates the route rows with the rule Model Plane's
// matcher will implement: a candidate is entitled when the policy permits it
// and, for a ZDR token, it is attested. A pinned model that the policy refuses
// reports the policy's reason (entitlement is named before any ZDR refusal);
// an unpinned route with nothing left is no_entitled_route.
func TestMatrixRouteRows(t *testing.T) {
	doc := loadMatrix(t)
	ran := 0
	for _, row := range doc.Rows {
		if row.Kind != "route" || !row.evaluatedByOrgCore() {
			continue
		}
		ran++
		t.Run(row.ID, func(t *testing.T) {
			var input struct {
				Zdr        bool   `json:"zdr"`
				Pinned     bool   `json:"pinned"`
				Policy     Policy `json:"policy"`
				Candidates []struct {
					Model    string `json:"model"`
					Provider string `json:"provider"`
					Attested bool   `json:"attested"`
				} `json:"candidates"`
			}
			var expect struct {
				Entitled []string `json:"entitled"`
				Reason   string   `json:"reason"`
			}
			strictDecode(t, row.Input, &input)
			strictDecode(t, row.Expect, &expect)

			entitled := []string{}
			firstDenial := ""
			for _, candidate := range input.Candidates {
				permitted, reason := input.Policy.Permits(candidate.Model, candidate.Provider)
				if !permitted {
					if firstDenial == "" {
						firstDenial = reason
					}
					continue
				}
				if input.Zdr && !candidate.Attested {
					continue
				}
				entitled = append(entitled, candidate.Model)
			}
			reason := ""
			if len(entitled) == 0 {
				reason = "no_entitled_route"
				if input.Pinned && firstDenial != "" {
					reason = firstDenial
				}
			}
			if !reflect.DeepEqual(entitled, expect.Entitled) || reason != expect.Reason {
				t.Fatalf("route = %v %q; want %v %q", entitled, reason, expect.Entitled, expect.Reason)
			}
		})
	}
	if ran == 0 {
		t.Fatal("no route rows were evaluated")
	}
}

func sortedKeys(t *testing.T, value any) []string {
	t.Helper()
	tree, ok := asTree(t, value).(map[string]any)
	if !ok {
		t.Fatalf("%T is not a JSON object", value)
	}
	keys := make([]string, 0, len(tree))
	for key := range tree {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}

// TestMatrixShapeRows pins the closed key sets of what org-core stores and
// serves: ids, enums and numbers only (z_policy_has_no_content). A fully
// populated value is marshalled, so a field added to the types without a
// reviewed matrix change fails here.
func TestMatrixShapeRows(t *testing.T) {
	doc := loadMatrix(t)
	ran := 0
	for _, row := range doc.Rows {
		if row.Kind != "shape" || !row.evaluatedByOrgCore() {
			continue
		}
		ran++
		t.Run(row.ID, func(t *testing.T) {
			var input struct {
				Subject string `json:"subject"`
			}
			strictDecode(t, row.Input, &input)
			policy := Policy{
				AllowMode: AllowModeListed, Allow: []string{"m"}, Deny: []string{"n"}, MaxEffort: "high", MinPrivacyTier: "global",
				Defaults: map[string]SurfaceDefault{"chat": {Model: "m", Effort: "low"}},
			}
			switch input.Subject {
			case "policy":
				var expect struct {
					Keys               []string `json:"keys"`
					SurfaceDefaultKeys []string `json:"surface_default_keys"`
				}
				strictDecode(t, row.Expect, &expect)
				if got := sortedKeys(t, policy); !reflect.DeepEqual(got, expect.Keys) {
					t.Fatalf("policy keys = %v; want %v", got, expect.Keys)
				}
				if got := sortedKeys(t, policy.Defaults["chat"]); !reflect.DeepEqual(got, expect.SurfaceDefaultKeys) {
					t.Fatalf("surface default keys = %v; want %v", got, expect.SurfaceDefaultKeys)
				}
			case "resolution":
				var expect struct {
					Keys       []string `json:"keys"`
					SourceKeys []string `json:"source_keys"`
				}
				strictDecode(t, row.Expect, &expect)
				resolution := Resolution{State: StateRole, Revision: 1, Role: "r", CanManage: true, Policy: &policy, Sources: []Source{{Layer: LayerRoleOverride, Role: "r"}}}
				if got := sortedKeys(t, resolution); !reflect.DeepEqual(got, expect.Keys) {
					t.Fatalf("resolution keys = %v; want %v", got, expect.Keys)
				}
				if got := sortedKeys(t, resolution.Sources[0]); !reflect.DeepEqual(got, expect.SourceKeys) {
					t.Fatalf("source keys = %v; want %v", got, expect.SourceKeys)
				}
			default:
				t.Fatalf("shape subject %q is not evaluated here", input.Subject)
			}
		})
	}
	if ran == 0 {
		t.Fatal("no shape rows were evaluated")
	}
}

// A row that claims org-core evaluates it must be a kind this package
// evaluates; otherwise the row would be a silent pass.
func TestMatrixOrgCoreRowsAreAllEvaluated(t *testing.T) {
	evaluated := map[string]bool{"resolve": true, "permit": true, "route": true, "shape": true}
	for _, row := range loadMatrix(t).Rows {
		if row.evaluatedByOrgCore() && !evaluated[row.Kind] {
			t.Fatalf("row %s claims org-core evaluates kind %q, which no test here does", row.ID, row.Kind)
		}
	}
}

type fakeStore struct {
	set        *PolicySet
	getErr     error
	putCalls   int
	putOrg     string
	putActor   string
	putExpect  int64
	putRequest PutRequest
}

func (f *fakeStore) Get(_ context.Context, _ string) (*PolicySet, error) {
	return f.set, f.getErr
}

func (f *fakeStore) Put(_ context.Context, orgID, actorID string, expected int64, req PutRequest) (*PolicySet, error) {
	f.putCalls++
	f.putOrg, f.putActor, f.putExpect, f.putRequest = orgID, actorID, expected, req
	return &PolicySet{OrganizationID: orgID, Revision: expected + 1, OrgDefault: req.OrgDefault, Roles: req.Roles}, nil
}

type fakeRoles struct {
	roles map[string]string
	err   error
	calls int
}

func (f *fakeRoles) CallerRole(_ context.Context, _, userID string) (string, error) {
	f.calls++
	return f.roles[userID], f.err
}

func TestServiceResolveReadsTheRoleFromTheMembershipSource(t *testing.T) {
	set := &PolicySet{Revision: 3, OrgDefault: &Policy{AllowMode: AllowModeAll, Allow: []string{}, Deny: []string{"gpt-5"}}, Roles: map[string]Policy{}}
	roles := &fakeRoles{roles: map[string]string{"u-admin": "admin"}}
	service := NewService(&fakeStore{set: set}, roles)

	admin, err := service.Resolve(context.Background(), "org-1", "u-admin")
	if err != nil {
		t.Fatalf("resolve admin: %v", err)
	}
	if admin.Role != "admin" || !admin.CanManage || admin.State != StateOrg {
		t.Fatalf("admin resolution = %+v", admin)
	}

	stranger, err := service.Resolve(context.Background(), "org-1", "u-stranger")
	if err != nil {
		t.Fatalf("resolve stranger: %v", err)
	}
	if stranger.State != StateMemberNotFound {
		t.Fatalf("a user who is not a member resolved to %q", stranger.State)
	}

	callsBefore := roles.calls
	serviceCall, err := service.Resolve(context.Background(), "org-1", "  ")
	if err != nil {
		t.Fatalf("resolve service call: %v", err)
	}
	if roles.calls != callsBefore || serviceCall.State != StateOrg || serviceCall.Role != "" {
		t.Fatalf("a call with no user must not look up a role and gets the default: %+v (%d role lookups)", serviceCall, roles.calls-callsBefore)
	}
}

func TestServiceResolveFailsWhenEitherSourceFails(t *testing.T) {
	boom := errors.New("boom")
	if _, err := NewService(&fakeStore{getErr: boom}, &fakeRoles{}).Resolve(context.Background(), "org-1", "u"); !errors.Is(err, boom) {
		t.Fatalf("store failure must surface, got %v", err)
	}
	if _, err := NewService(&fakeStore{set: &PolicySet{}}, &fakeRoles{err: boom}).Resolve(context.Background(), "org-1", "u"); !errors.Is(err, boom) {
		t.Fatalf("role lookup failure must surface and never read as unrestricted, got %v", err)
	}
}

func TestServicePutValidatesBeforeTouchingTheStore(t *testing.T) {
	store := &fakeStore{}
	service := NewService(store, &fakeRoles{})
	zero := int64(0)

	_, err := service.Put(context.Background(), "org-1", "u-owner", PutRequest{ExpectedRevision: &zero, OrgDefault: &Policy{AllowMode: "everything"}})
	var invalid *ValidationError
	if !errors.As(err, &invalid) || store.putCalls != 0 {
		t.Fatalf("an invalid body must be a ValidationError and write nothing: err=%v writes=%d", err, store.putCalls)
	}
	if _, err := service.Put(context.Background(), "org-1", " ", PutRequest{ExpectedRevision: &zero}); err == nil || store.putCalls != 0 {
		t.Fatalf("a write with no actor must be refused: err=%v writes=%d", err, store.putCalls)
	}

	saved, err := service.Put(context.Background(), "org-1", "u-owner", PutRequest{
		ExpectedRevision: &zero,
		OrgDefault:       &Policy{AllowMode: " LISTED ", Allow: []string{"GPT-5-Mini", "gpt-5-mini"}},
	})
	if err != nil {
		t.Fatalf("valid put: %v", err)
	}
	if store.putOrg != "org-1" || store.putActor != "u-owner" || store.putExpect != 0 || saved.Revision != 1 {
		t.Fatalf("store got org=%q actor=%q expected=%d, saved revision %d", store.putOrg, store.putActor, store.putExpect, saved.Revision)
	}
	if got := store.putRequest.OrgDefault.Allow; !reflect.DeepEqual(got, []string{"gpt-5-mini"}) || store.putRequest.OrgDefault.AllowMode != AllowModeListed {
		t.Fatalf("the store must receive the normalised policy, got %+v", store.putRequest.OrgDefault)
	}
}
