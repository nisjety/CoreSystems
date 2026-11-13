package api

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"io/fs"
	"net/http"
	"net/http/httptest"
	"os"
	"path"
	"strings"
	"testing"
	"testing/fstest"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/zdr"
)

// The posture matrix (ZDR-Z-12-13 step 8). integration-corev2 owns the acknowledgement states of the
// matrix's vocabulary.acks: for every decision row whose connection is own, other_member or other_org
// and whose outage is null, the ledger state the row's ack names is built in the memory store and the
// real GET /api/v1/model-connections/:id/zdr-posture is called through the fiber app as the session
// user. A row names what Auth Core must see; nothing here re-implements the decision.
//
// testdata/zdr-posture-matrix.json is a byte copy of apps/Model Plane/api/contracts/zdr-posture-matrix.json
// pinned by its .sha256 (LF-normalised sha256, written "hash *name").

const (
	matrixFile     = "testdata/zdr-posture-matrix.json"
	matrixPinFile  = "testdata/zdr-posture-matrix.json.sha256"
	matrixOrg      = "org-matrix"
	matrixOther    = "org-elsewhere"
	matrixOwner    = "user-owner"
	matrixMember   = "user-member"
	matrixOutsider = "user-outsider"
	matrixConn     = "conn-matrix"
	matrixConnTwo  = "conn-matrix-2"
	matrixConnPeer = "conn-matrix-peer"
	// matrixAuthModeOther is an auth mode the capability contract has no row for: the contract answers
	// with its fallback (cannot_verify, no waiver route).
	matrixAuthModeOther = "api_key"
)

type matrixDecisionInputs struct {
	OrgZDRIntent  *bool   `json:"org_zdr_intent"`
	OrgEntitled   *bool   `json:"org_entitled"`
	ExplicitZDR   bool    `json:"explicit_zdr"`
	SpaceZDR      bool    `json:"space_zdr"`
	WaiverEnabled bool    `json:"waiver_enabled"`
	Connection    string  `json:"connection"`
	Capability    *string `json:"capability"`
	WaiverRoute   *bool   `json:"waiver_route"`
	Ack           string  `json:"ack"`
	Outage        *string `json:"outage"`
}

type matrixDecisionRow struct {
	ID     string
	Inputs matrixDecisionInputs
}

type matrixDocument struct {
	Contract   string
	Vocabulary struct {
		Acks map[string]string `json:"acks"`
	} `json:"vocabulary"`
	DecisionRows []matrixDecisionRow
}

// loadPostureMatrix reads the vendored matrix, checks it against its pin and decodes the decision rows.
// The inputs of a decision row are decoded strictly: a field this test does not know is a matrix change
// it has not been taught, and fails here rather than being ignored.
func loadPostureMatrix(t *testing.T) matrixDocument {
	t.Helper()
	raw, err := os.ReadFile(matrixFile)
	if err != nil {
		t.Fatalf("read %s: %v", matrixFile, err)
	}
	pinRaw, err := os.ReadFile(matrixPinFile)
	if err != nil {
		t.Fatalf("read %s: %v", matrixPinFile, err)
	}
	pinFields := strings.Fields(string(pinRaw))
	if len(pinFields) != 2 || strings.TrimPrefix(pinFields[1], "*") != "zdr-posture-matrix.json" {
		t.Fatalf("%s must be `<sha256> *zdr-posture-matrix.json`, got %q", matrixPinFile, string(pinRaw))
	}
	sum := sha256.Sum256(bytes.ReplaceAll(raw, []byte("\r\n"), []byte("\n")))
	if got := hex.EncodeToString(sum[:]); got != pinFields[0] {
		t.Fatalf("the vendored posture matrix does not match its pin (pinned %s, actual %s): re-vendor it with its .sha256", pinFields[0], got)
	}

	var envelope struct {
		Contract   string          `json:"contract"`
		Vocabulary json.RawMessage `json:"vocabulary"`
		Decision   []struct {
			ID     string          `json:"id"`
			Inputs json.RawMessage `json:"inputs"`
		} `json:"decision_rows"`
	}
	if err := json.Unmarshal(raw, &envelope); err != nil {
		t.Fatalf("decode posture matrix: %v", err)
	}
	var doc matrixDocument
	doc.Contract = envelope.Contract
	if err := json.Unmarshal(envelope.Vocabulary, &doc.Vocabulary); err != nil {
		t.Fatalf("decode vocabulary: %v", err)
	}
	for _, row := range envelope.Decision {
		decoder := json.NewDecoder(bytes.NewReader(row.Inputs))
		decoder.DisallowUnknownFields()
		var inputs matrixDecisionInputs
		if err := decoder.Decode(&inputs); err != nil {
			t.Fatalf("decision row %s: inputs do not decode strictly: %v", row.ID, err)
		}
		doc.DecisionRows = append(doc.DecisionRows, matrixDecisionRow{ID: row.ID, Inputs: inputs})
	}
	if doc.Contract != "zdr.posture-matrix" || len(doc.DecisionRows) == 0 {
		t.Fatalf("the vendored file is not the posture matrix (contract %q, %d decision rows)", doc.Contract, len(doc.DecisionRows))
	}
	return doc
}

// matrixWording is the embedded v1 wording plus an identical v2, with v2 current and the minimum.
// Only an injected wording can make text_version_below_minimum reachable: the embedded index has one
// version. The texts are the shipped ones; only the version they are filed under differs.
func matrixWording(t *testing.T) *zdr.Wording {
	t.Helper()
	return matrixWordingWithMinimum(t, "v2")
}

// matrixWordingWithMinimum is the same two-version wording with v2 current and the given minimum, v1 or v2. With
// v1 as the minimum, an acknowledgement of v1 stays in force while v2 is current.
func matrixWordingWithMinimum(t *testing.T, minimum string) *zdr.Wording {
	t.Helper()
	assessor, err := zdr.Default()
	if err != nil {
		t.Fatalf("zdr.Default: %v", err)
	}
	source := os.DirFS("../zdr")
	files := fstest.MapFS{}
	v1Pins := map[string]string{}
	v2Pins := map[string]string{}
	err = fs.WalkDir(source, "texts/v1", func(p string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil || entry.IsDir() {
			return walkErr
		}
		data, readErr := fs.ReadFile(source, p)
		if readErr != nil {
			return readErr
		}
		sum := sha256.Sum256(bytes.ReplaceAll(data, []byte("\r\n"), []byte("\n")))
		name := strings.TrimPrefix(p, "texts/")
		v2Name := "v2/" + strings.TrimPrefix(name, "v1/")
		files[p] = &fstest.MapFile{Data: data}
		files[path.Join("texts", v2Name)] = &fstest.MapFile{Data: data}
		v1Pins[name] = hex.EncodeToString(sum[:])
		v2Pins[v2Name] = hex.EncodeToString(sum[:])
		return nil
	})
	if err != nil {
		t.Fatalf("copy the embedded wording: %v", err)
	}
	index, err := json.Marshal(map[string]any{
		"format":  "zdr-acknowledgement-wording/1",
		"current": "v2",
		"minimum": minimum,
		"versions": map[string]any{
			"v1": map[string]any{"status": "superseded", "files": v1Pins},
			"v2": map[string]any{"status": "draft_pending_od_legal", "files": v2Pins},
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	files["texts/index.json"] = &fstest.MapFile{Data: index}
	wording, err := zdr.LoadWording(files, assessor)
	if err != nil {
		t.Fatalf("load the v1+v2 wording: %v", err)
	}
	if wording.Version() != "v2" || wording.MinimumVersion() != minimum {
		t.Fatalf("injected wording is %s with minimum %s, want v2 and %s", wording.Version(), wording.MinimumVersion(), minimum)
	}
	return wording
}

// matrixRig serves the real routes over one memory repository for any number of principals.
type matrixRig struct {
	t       *testing.T
	repo    *store.MemoryRepository
	wording *zdr.Wording
	apps    map[string]*fiber.App
}

func newMatrixRig(t *testing.T, wording *zdr.Wording) *matrixRig {
	t.Helper()
	_, repo, _ := testOAuthStack(t)
	return &matrixRig{t: t, repo: repo, wording: wording, apps: map[string]*fiber.App{}}
}

func (r *matrixRig) app(principal auth.Principal) *fiber.App {
	r.t.Helper()
	key := principal.OrganizationID + "/" + principal.UserID
	if app, ok := r.apps[key]; ok {
		return app
	}
	cfg, _, service := testOAuthStack(r.t)
	cfg.ZDRAcknowledgementEnabled = true
	app := NewServer(ServerConfig{
		Config: cfg, Repo: r.repo, OAuth: service, Events: &fakeEventsPublisher{},
		OrgZDR: &fakeOrgPosture{on: true}, Auth: fakeVerifier{principal: principal}, ZDRWording: r.wording,
	})
	r.apps[key] = app
	return app
}

func (r *matrixRig) call(principal auth.Principal, method, target, body string) (int, http.Header, map[string]any) {
	r.t.Helper()
	req := httptest.NewRequest(method, target, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer token-1")
	resp, err := r.app(principal).Test(req)
	if err != nil {
		r.t.Fatalf("app.Test %s %s: %v", method, target, err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	var decoded map[string]any
	_ = json.Unmarshal(raw, &decoded)
	return resp.StatusCode, resp.Header, decoded
}

// connect stores a ChatGPT subscription connection and records the assessment named by capability and
// waiverRoute. not_supported with a waiver route, and cannot_verify without one, are what the shipped
// contract assesses (the subscription row and its fallback). supported, and cannot_verify with a waiver
// route, are rows the contract has no provider for yet; they are recorded as a contract row would record
// them (a digest of their own fields), so the posture read is tested on every capability the matrix names.
func (r *matrixRig) connect(id, org, user, capability string, waiverRoute bool, catalogVersion int) string {
	r.t.Helper()
	ctx := r.t.Context()
	if _, err := r.repo.UpsertConnection(ctx, store.Connection{
		ID: id, ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType,
		OrganizationID: org, WorkspaceID: "ws-" + org, UserID: user, Status: "active",
		Capabilities: []string{codexsubscription.Capability},
	}); err != nil {
		r.t.Fatalf("seed connection %s: %v", id, err)
	}
	authMode, model := modelConnectionAuthMode(store.Connection{ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType})
	if !model {
		r.t.Fatal("the subscription provider is no longer a model connection")
	}
	if !waiverRoute {
		authMode = matrixAuthModeOther
	}
	return r.assess(id, authMode, zdr.Capability(capability), catalogVersion)
}

// assess records an assessment for the connection and returns its digest.
func (r *matrixRig) assess(id, authMode string, capability zdr.Capability, catalogVersion int) string {
	r.t.Helper()
	assessor, err := zdr.Default()
	if err != nil {
		r.t.Fatal(err)
	}
	contract, err := assessor.Assess(codexsubscription.ProviderKey, authMode)
	if err != nil {
		r.t.Fatalf("assess %s: %v", authMode, err)
	}
	assessment := store.NewConnectionZDRAssessment(id, contract)
	if capability != contract.Capability || catalogVersion != contract.CatalogVersion {
		assessment.Capability = capability
		assessment.CatalogVersion = catalogVersion
		if capability == zdr.CapabilitySupported {
			assessment.Basis = zdr.BasisProviderByContract
		} else if capability == zdr.CapabilityCannotVerify {
			assessment.Basis = zdr.BasisNotAssessed
		}
		assessment.BasisRef = "matrix-test@" + string(capability)
		digest, digestErr := zdr.Digest(assessment.ProviderKey, assessment.AuthMode, assessment.Capability, assessment.Basis, assessment.CatalogVersion)
		if digestErr != nil {
			r.t.Fatal(digestErr)
		}
		assessment.Digest = digest
	}
	if _, err := r.repo.ApplyConnectionZDRAssessment(r.t.Context(), assessment); err != nil {
		r.t.Fatalf("record assessment of %s: %v", id, err)
	}
	return assessment.Digest
}

func (r *matrixRig) digest(id string) string {
	r.t.Helper()
	record, err := r.repo.GetConnectionZDR(r.t.Context(), id)
	if err != nil {
		r.t.Fatalf("record of %s: %v", id, err)
	}
	return record.Digest
}

// acknowledge acknowledges through the real route as the connection's owner, on the current text.
func (r *matrixRig) acknowledge(owner auth.Principal, id string) (int, string) {
	r.t.Helper()
	status, _, body := r.call(owner, http.MethodPost, ackPath(id)+"/zdr-acknowledgement",
		ackBody(r.digest(id), r.wording.Version(), "no", "connect"))
	if status != http.StatusCreated && status != http.StatusOK {
		return status, ""
	}
	ack, _ := dataOf(body)["acknowledgement"].(map[string]any)
	ackID, _ := ack["id"].(string)
	return status, ackID
}

// acknowledgeOldText records an acknowledgement given against the v1 text, as one recorded before the
// minimum moved to v2 would be: the v1 snapshot, its hash and text version v1.
func (r *matrixRig) acknowledgeOldText(org, user, id string) {
	r.t.Helper()
	record, err := r.repo.GetConnectionZDR(r.t.Context(), id)
	if err != nil {
		r.t.Fatal(err)
	}
	key, err := zdrWordingKey(record)
	if err != nil {
		r.t.Fatal(err)
	}
	v1, err := zdr.DefaultWording()
	if err != nil {
		r.t.Fatal(err)
	}
	text, err := v1.Lookup(key, "org_zdr_on", "no")
	if err != nil {
		r.t.Fatal(err)
	}
	if text.Version != "v1" {
		r.t.Fatalf("the embedded wording is %s; this test assumes the shipped text is v1", text.Version)
	}
	if _, created, err := r.repo.InsertConnectionZDRAcknowledgement(r.t.Context(), store.NewConnectionZDRAcknowledgement{
		OrganizationID: org, ConnectionID: id, UserID: user, Digest: record.Digest,
		TextVersion: text.Version, Locale: "no", TextSHA256: text.SHA256, TextSnapshot: text.Snapshot,
		OrgZDRAtAck: true, AcknowledgedVia: store.ZDRAcknowledgedViaConnect,
	}); err != nil || !created {
		r.t.Fatalf("record a v1 acknowledgement: created %v, %v", created, err)
	}
}

func (r *matrixRig) posture(principal auth.Principal, id string) (int, map[string]any, map[string]any) {
	r.t.Helper()
	status, header, body := r.call(principal, http.MethodGet, ackPath(id)+"/zdr-posture", "")
	if status == http.StatusOK && header.Get("Cache-Control") != "no-store" {
		r.t.Fatalf("posture read of %s answered Cache-Control %q, want no-store", id, header.Get("Cache-Control"))
	}
	return status, dataOf(body), body
}

func matrixPrincipal(org, user string) auth.Principal {
	return auth.Principal{UserID: user, OrganizationID: org, PrincipalType: "user", Role: "member"}
}

func TestEveryPostureMatrixAcknowledgementStateIsWhatThePostureReadAnswers(t *testing.T) {
	doc := loadPostureMatrix(t)
	wording := matrixWording(t)
	owner := matrixPrincipal(matrixOrg, matrixOwner)
	member := matrixPrincipal(matrixOrg, matrixMember)
	outsider := matrixPrincipal(matrixOther, matrixOutsider)

	acksSeen := map[string]bool{}
	connectionsSeen := map[string]bool{}
	checked := 0
	for _, row := range doc.DecisionRows {
		in := row.Inputs
		if in.Outage != nil {
			continue
		}
		if in.Connection != "own" && in.Connection != "other_member" && in.Connection != "other_org" {
			continue
		}
		if in.Capability == nil || in.WaiverRoute == nil {
			t.Fatalf("row %s names a connection but no capability or waiver route", row.ID)
		}
		if _, known := doc.Vocabulary.Acks[in.Ack]; !known {
			t.Fatalf("row %s names ack %q, which vocabulary.acks does not define", row.ID, in.Ack)
		}
		capability := *in.Capability
		if !zdr.Capability(capability).Valid() {
			t.Fatalf("row %s names capability %q, which this service does not know", row.ID, capability)
		}
		checked++
		acksSeen[in.Ack] = true
		connectionsSeen[in.Connection] = true

		t.Run(row.ID, func(t *testing.T) {
			rig := newMatrixRig(t, wording)
			rig.t = t
			rig.connect(matrixConn, matrixOrg, matrixOwner, capability, *in.WaiverRoute, 1)
			supported := zdr.Capability(capability).KeepsZDR()

			// Build the ledger the row's ack names. A supported connection takes no acknowledgement at
			// all: the route refuses it, so the ledger stays empty whatever the row says.
			wantAckID := ""
			switch in.Ack {
			case "none":
			case "valid", "revoked", "stale_digest":
				if in.Connection == "own" && supported {
					if status, _ := rig.acknowledge(owner, matrixConn); status != http.StatusConflict {
						t.Fatalf("acknowledging a supported connection answered %d, want 409", status)
					}
					break
				}
				status, ackID := rig.acknowledge(owner, matrixConn)
				if status != http.StatusCreated || ackID == "" {
					t.Fatalf("owner acknowledgement answered %d", status)
				}
				wantAckID = ackID
				switch in.Ack {
				case "revoked":
					if status, _, body := rig.call(owner, http.MethodDelete, ackPath(matrixConn)+"/zdr-acknowledgement", ""); status != http.StatusOK {
						t.Fatalf("revoke answered %d %v", status, body)
					}
				case "stale_digest":
					// The catalogue moved: the same capability under a new catalogue version is a new digest.
					authMode := matrixAuthModeOther
					if *in.WaiverRoute {
						authMode, _ = modelConnectionAuthMode(store.Connection{ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType})
					}
					before := rig.digest(matrixConn)
					rig.assess(matrixConn, authMode, zdr.Capability(capability), 2)
					if rig.digest(matrixConn) == before {
						t.Fatal("a new catalogue version kept the digest; the stale_digest state was not built")
					}
				}
			case "text_version_below_minimum":
				rig.acknowledgeOldText(matrixOrg, matrixOwner, matrixConn)
			case "other_connection":
				rig.connect(matrixConnTwo, matrixOrg, matrixOwner, capability, *in.WaiverRoute, 1)
				if status, _ := rig.acknowledge(owner, matrixConnTwo); status != http.StatusCreated {
					t.Fatalf("acknowledging the session user's other connection answered %d", status)
				}
			case "other_member":
				// Another member of the organisation holds a valid acknowledgement for the same provider
				// and digest on their own connection. When the session user is not the owner, the owner's
				// own valid acknowledgement of this very connection is the other member's too.
				rig.connect(matrixConnPeer, matrixOrg, matrixMember, capability, *in.WaiverRoute, 1)
				if !supported {
					if status, _ := rig.acknowledge(member, matrixConnPeer); status != http.StatusCreated {
						t.Fatalf("the other member's acknowledgement answered %d", status)
					}
					if rig.digest(matrixConnPeer) != rig.digest(matrixConn) {
						t.Fatal("the other member's connection has another digest; the row's state was not built")
					}
					if in.Connection == "other_member" {
						if status, _ := rig.acknowledge(owner, matrixConn); status != http.StatusCreated {
							t.Fatalf("the owner's acknowledgement answered %d", status)
						}
					}
				}
			default:
				t.Fatalf("row %s names ack %q, which this test cannot build", row.ID, in.Ack)
			}

			switch in.Connection {
			case "other_member":
				status, data, body := rig.posture(member, matrixConn)
				if status != http.StatusForbidden || errorCode(body) != "not_connection_owner" || data != nil {
					t.Fatalf("another member read the owner's posture: %d %v, want 403 not_connection_owner and no data", status, body)
				}
				return
			case "other_org":
				status, data, body := rig.posture(outsider, matrixConn)
				if status != http.StatusForbidden || errorCode(body) != "forbidden" || data != nil {
					t.Fatalf("a member of another organisation read the posture: %d %v, want 403 forbidden and no data", status, body)
				}
				return
			}

			status, data, body := rig.posture(owner, matrixConn)
			if status != http.StatusOK {
				t.Fatalf("owner posture read: %d %v", status, body)
			}
			assertPostureKeys(t, data)
			if data["capability"] != capability || data["waiverRoute"] != *in.WaiverRoute {
				t.Fatalf("posture carries capability %v waiverRoute %v, want %s and %v", data["capability"], data["waiverRoute"], capability, *in.WaiverRoute)
			}
			if data["providerKey"] != codexsubscription.ProviderKey || data["connectionId"] != matrixConn || data["digest"] != rig.digest(matrixConn) {
				t.Fatalf("posture names the wrong connection, provider or digest: %v", data)
			}
			wantState, wantReason := "required", in.Ack
			switch {
			case supported:
				wantState, wantReason = "not_required", ""
			case in.Ack == "valid":
				wantState, wantReason = "acknowledged", ""
			case in.Ack == "other_connection" || in.Ack == "other_member":
				wantReason = "none"
			}
			if data["ackState"] != wantState || data["ackReason"] != wantReason {
				t.Fatalf("ack %s on a %s connection: posture (ackState %v, ackReason %v), want (%s, %q)", in.Ack, capability, data["ackState"], data["ackReason"], wantState, wantReason)
			}
			if wantState == "acknowledged" {
				if data["ackId"] != wantAckID || data["textVersion"] != "v2" {
					t.Fatalf("acknowledged posture binds ackId %v textVersion %v, want %s and v2", data["ackId"], data["textVersion"], wantAckID)
				}
			} else if data["ackId"] != "" {
				t.Fatalf("a posture that is not acknowledged carries ackId %v", data["ackId"])
			}
		})
	}

	if checked == 0 {
		t.Fatal("no decision row names a connection without an outage; the matrix lost the rows this service owns")
	}
	for ack := range doc.Vocabulary.Acks {
		if !acksSeen[ack] {
			t.Errorf("no decision row exercises ack %q, so this service's answer for it is untested", ack)
		}
	}
	for _, connection := range []string{"own", "other_member", "other_org"} {
		if !connectionsSeen[connection] {
			t.Errorf("no decision row exercises connection %q", connection)
		}
	}
}

// The reason codes have an order: an active row below the minimum, then a revoked row on the current
// digest, then any row on another digest. Each later state is built on top of the earlier ones.
func TestThePostureReasonPrefersBelowMinimumThenRevokedThenStale(t *testing.T) {
	rig := newMatrixRig(t, matrixWording(t))
	owner := matrixPrincipal(matrixOrg, matrixOwner)
	rig.connect(matrixConn, matrixOrg, matrixOwner, string(zdr.CapabilityNotSupported), true, 1)

	reason := func(want string) {
		t.Helper()
		status, data, body := rig.posture(owner, matrixConn)
		if status != http.StatusOK || data["ackState"] != "required" || data["ackReason"] != want {
			t.Fatalf("posture = %d %v, want required with ackReason %q", status, body, want)
		}
	}
	reason("none")

	if status, _ := rig.acknowledge(owner, matrixConn); status != http.StatusCreated {
		t.Fatalf("acknowledge: %d", status)
	}
	authMode, _ := modelConnectionAuthMode(store.Connection{ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType})
	rig.assess(matrixConn, authMode, zdr.CapabilityNotSupported, 2)
	reason("stale_digest")

	if status, _ := rig.acknowledge(owner, matrixConn); status != http.StatusCreated {
		t.Fatalf("acknowledge the new digest: %d", status)
	}
	if status, _, _ := rig.call(owner, http.MethodDelete, ackPath(matrixConn)+"/zdr-acknowledgement", ""); status != http.StatusOK {
		t.Fatalf("revoke: %d", status)
	}
	reason("revoked")

	rig.acknowledgeOldText(matrixOrg, matrixOwner, matrixConn)
	reason("text_version_below_minimum")
}

// Raising the minimum is how a changed text is made to count: every member whose acknowledgement is on an
// older text is asked again. Acknowledging the current text must then put an acknowledgement in force. An
// active row below the minimum is not an acknowledgement in force (the posture read says so), so it can
// neither answer the new acknowledgement as a replay nor be shown as the acknowledgement in force.
func TestAcknowledgingTheCurrentTextReplacesAnAcknowledgementBelowTheMinimum(t *testing.T) {
	rig := newMatrixRig(t, matrixWording(t))
	owner := matrixPrincipal(matrixOrg, matrixOwner)
	rig.connect(matrixConn, matrixOrg, matrixOwner, string(zdr.CapabilityNotSupported), true, 1)
	rig.acknowledgeOldText(matrixOrg, matrixOwner, matrixConn)

	_, data, _ := rig.posture(owner, matrixConn)
	if data["ackState"] != "required" || data["ackReason"] != "text_version_below_minimum" {
		t.Fatalf("posture with only a v1 acknowledgement = %v, want required text_version_below_minimum", data)
	}

	// What the member is shown before acknowledging must not claim an acknowledgement is in force.
	status, _, body := rig.call(owner, http.MethodGet, ackPath(matrixConn)+"/zdr", "")
	if status != http.StatusOK {
		t.Fatalf("assessment read: %d %v", status, body)
	}
	if inForce := dataOf(body)["acknowledgement"]; inForce != nil {
		t.Errorf("the assessment read shows %v as the acknowledgement in force although its text is below the minimum; the member is never asked again", inForce)
	}

	status, _, body = rig.call(owner, http.MethodPost, ackPath(matrixConn)+"/zdr-acknowledgement",
		ackBody(rig.digest(matrixConn), "v2", "no", "settings"))
	ack, _ := dataOf(body)["acknowledgement"].(map[string]any)
	if status != http.StatusCreated || ack["textVersion"] != "v2" {
		t.Errorf("acknowledging the v2 text answered %d with acknowledgement %v; want 201 and a new v2 acknowledgement, not the v1 row as a replay", status, ack)
	}
	_, data, _ = rig.posture(owner, matrixConn)
	if data["ackState"] != "acknowledged" || data["textVersion"] != "v2" {
		t.Fatalf("after acknowledging the current text the posture is %v, want acknowledged on v2: the member cannot get out of text_version_below_minimum", data)
	}
}

// The vendored copy is the contract's copy, byte for byte. Its pin is the contract's pin.
func TestTheVendoredPostureMatrixIsTheContractsCopy(t *testing.T) {
	const contractDir = "../../../../Model Plane/api/contracts/"
	for _, name := range []string{"zdr-posture-matrix.json", "zdr-posture-matrix.json.sha256"} {
		want, err := os.ReadFile(contractDir + name)
		if err != nil {
			t.Skipf("the Model Plane contract is not in this checkout: %v", err)
		}
		got, err := os.ReadFile("testdata/" + name)
		if err != nil {
			t.Fatal(err)
		}
		norm := func(b []byte) []byte { return bytes.ReplaceAll(b, []byte("\r\n"), []byte("\n")) }
		if !bytes.Equal(norm(want), norm(got)) {
			t.Fatalf("testdata/%s differs from the Model Plane contract: re-vendor it", name)
		}
	}
}
