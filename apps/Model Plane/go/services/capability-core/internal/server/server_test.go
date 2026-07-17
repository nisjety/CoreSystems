package server

import (
	"context"
	"errors"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/lettatools"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

type toolSearchStub struct {
	matches  []lettatools.Match
	err      error
	calls    int
	gotQuery string
	gotLimit int
}

type rankingTrailerStream struct {
	trailer metadata.MD
}

func (*rankingTrailerStream) Method() string {
	return mpv1.CapabilityCore_ListCapabilities_FullMethodName
}
func (*rankingTrailerStream) SetHeader(metadata.MD) error  { return nil }
func (*rankingTrailerStream) SendHeader(metadata.MD) error { return nil }
func (stream *rankingTrailerStream) SetTrailer(value metadata.MD) error {
	stream.trailer = metadata.Join(stream.trailer, value)
	return nil
}

func (stub *toolSearchStub) Search(_ context.Context, query string, limit int) ([]lettatools.Match, error) {
	stub.calls++
	stub.gotQuery = query
	stub.gotLimit = limit
	return append([]lettatools.Match(nil), stub.matches...), stub.err
}

type capabilityStoreStub struct {
	row        *registry.CapabilityRow
	scored     []registry.ScoredCapability
	err        error
	gotID      string
	gotOrg     string
	gotRankOrg string
}

type capabilitySourceStub struct {
	items []*models.Capability
}

func (source capabilitySourceStub) Load() ([]*models.Capability, error) {
	items := make([]*models.Capability, len(source.items))
	for index, capability := range source.items {
		copy := *capability
		items[index] = &copy
	}
	return items, nil
}

func (store *capabilityStoreStub) GetForOrg(_ context.Context, capabilityID, organizationID string) (*registry.CapabilityRow, error) {
	store.gotID = capabilityID
	store.gotOrg = organizationID
	return store.row, store.err
}

func (store *capabilityStoreStub) RankedList(_ context.Context, organizationID, _ string, _ []string, _ int) ([]registry.ScoredCapability, error) {
	store.gotRankOrg = organizationID
	return store.scored, store.err
}

func TestCapabilityDetailMappersPreserveFailClosedAvailabilityContract(t *testing.T) {
	t.Parallel()

	if rowToDetail(nil) != nil || toDetail(nil) != nil {
		t.Fatal("nil capability must remain nil")
	}
	checkedAt := time.Now().UTC()
	detail := rowToDetail(&registry.CapabilityRow{
		ID:                "cap.shipping.quote",
		Name:              "Shipping quote",
		Kind:              "tool",
		Version:           "1",
		Description:       "Read-only quote",
		RiskLevel:         "high",
		LazyLoad:          true,
		Scope:             "tenant",
		Enabled:           true,
		AvailabilityState: "available",
		ReasonCode:        "runtime_healthy",
		Reason:            "probe succeeded",
		ExecutionMode:     "agentic",
		CostClass:         "variable",
		HealthCheckedAt:   &checkedAt,
	})

	if detail.CapabilityId != "cap.shipping.quote" || detail.State != "approval_required" {
		t.Fatalf("detail identity/state = %+v", detail)
	}
	if !detail.RequiresApproval || detail.ExecutionMode != "agentic" || detail.CostClass != "variable" {
		t.Fatalf("detail policy = %+v", detail)
	}
	if detail.HealthCheckedAt == "" || detail.ReasonCode != "runtime_healthy" {
		t.Fatalf("detail health = %+v", detail)
	}
}

func TestCapabilityDetailMapperNeverAdvertisesQuarantinedRollout(t *testing.T) {
	t.Parallel()
	checkedAt := time.Now().UTC()
	detail := rowToDetail(&registry.CapabilityRow{
		ID: "cap.quarantined", Enabled: true, RiskLevel: models.RiskLow,
		RolloutState: "quarantine", AvailabilityState: "available",
		ExecutionMode: models.ExecutionDirectRead, HealthCheckedAt: &checkedAt,
	})
	if detail.State != string(models.AvailabilityUnavailable) || detail.ReasonCode != "rollout_quarantine" {
		t.Fatalf("quarantined detail = %+v", detail)
	}
}

func TestGetCapabilityUsesAuthoritativeDurableAvailabilityWhenStoreIsAttached(t *testing.T) {
	t.Parallel()

	checkedAt := time.Now().UTC()
	server := newTestServer()
	store := &capabilityStoreStub{row: &registry.CapabilityRow{
		ID:                "cap.read",
		Enabled:           true,
		RiskLevel:         "low",
		AvailabilityState: "available",
		ReasonCode:        "runtime_healthy",
		ExecutionMode:     "direct_read",
		CostClass:         "bounded",
		HealthCheckedAt:   &checkedAt,
	}}
	server.store = store

	ctx := verifiedGRPCContext(t, "triodelab")
	detail, err := server.GetCapability(ctx, &mpv1.GetCapabilityRequest{CapabilityId: "cap.read"})
	if err != nil {
		t.Fatal(err)
	}
	if detail.State != "available" || detail.ExecutionMode != "direct_read" || detail.HealthCheckedAt == "" {
		t.Fatalf("detail = %+v", detail)
	}
	if store.gotID != "cap.read" || store.gotOrg != "triodelab" {
		t.Fatalf("durable lookup = id %q org %q", store.gotID, store.gotOrg)
	}

	_, err = server.GetCapability(ctx, &mpv1.GetCapabilityRequest{
		CapabilityId: "cap.read", VersionConstraint: "2",
	})
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("version mismatch status = %v", err)
	}

	server.store = &capabilityStoreStub{err: errors.New("database unavailable")}
	_, err = server.GetCapability(ctx, &mpv1.GetCapabilityRequest{CapabilityId: "cap.read"})
	if status.Code(err) != codes.Internal {
		t.Fatalf("store error status = %v", err)
	}
}

func TestListCapabilitiesPinsDurableRankingToVerifiedTenant(t *testing.T) {
	t.Parallel()

	server := newTestServer()
	store := &capabilityStoreStub{scored: []registry.ScoredCapability{{
		Row: &registry.CapabilityRow{
			ID: "cap.owned", OrgID: "org-a", Name: "Owned", Kind: models.KindTool,
			Version: "1", RiskLevel: models.RiskLow, Enabled: true,
		},
	}}}
	server.store = store

	response, err := server.ListCapabilities(
		verifiedGRPCContext(t, "org-a"),
		&mpv1.ListCapabilitiesRequest{},
	)
	if err != nil {
		t.Fatal(err)
	}
	if store.gotRankOrg != "org-a" {
		t.Fatalf("durable ranked tenant = %q", store.gotRankOrg)
	}
	if len(response.Capabilities) != 1 || response.Capabilities[0].CapabilityId != "cap.owned" {
		t.Fatalf("ranked response = %+v", response)
	}
}

func TestListCapabilitiesLettaRanksOnlyExactLocalDispatchIntersection(t *testing.T) {
	t.Parallel()

	server := newTestServer()
	server.store = &capabilityStoreStub{scored: []registry.ScoredCapability{
		{Row: &registry.CapabilityRow{
			ID: "cap.track", OrgID: "org-a", Name: "Shipment lookup", Kind: models.KindTool,
			Version: "1", Description: "Read an exact carrier state", RiskLevel: models.RiskLow, Enabled: true,
			ConfigJSON: []byte(`{"dispatch_name":"track_shipment"}`),
		}},
		{Row: &registry.CapabilityRow{
			ID: "cap.book", OrgID: "global", Name: "Shipment booking", Kind: models.KindTool,
			Version: "1", Description: "Create a shipment", RiskLevel: models.RiskHigh, Enabled: true,
			ConfigJSON: []byte(`{"dispatch_name":"book_shipment"}`),
		}},
		{Row: &registry.CapabilityRow{
			ID: "cap.admin", OrgID: "org-a", Name: "Administration", Kind: models.KindTool,
			Version: "1", Description: "Unrelated local authority", RiskLevel: models.RiskHigh, Enabled: true,
			ConfigJSON: []byte(`{"dispatch_name":"admin_delete_everything"}`),
		}},
	}}
	searcher := &toolSearchStub{matches: []lettatools.Match{
		{Name: "book_shipment"},
		{Name: "remote_only_tool"},
		{Name: "track_shipment"},
	}}
	server.WithLettaToolSearcher(searcher)
	nonZDR := false

	response, err := server.ListCapabilities(
		verifiedGRPCContextWithRetention(t, "org-a", &nonZDR),
		&mpv1.ListCapabilitiesRequest{Query: "shipment", Limit: 20},
	)
	if err != nil {
		t.Fatal(err)
	}
	if searcher.calls != 1 || searcher.gotQuery != "shipment" || searcher.gotLimit < 1 || searcher.gotLimit > 100 {
		t.Fatalf("Letta search call = %+v", searcher)
	}
	if len(response.Capabilities) != 2 {
		t.Fatalf("capabilities = %+v", response.Capabilities)
	}
	if response.Capabilities[0].CapabilityId != "cap.book" || response.Capabilities[1].CapabilityId != "cap.track" {
		t.Fatalf("ranked local intersection = %+v", response.Capabilities)
	}
	for _, capability := range response.Capabilities {
		if capability.CapabilityId == "remote_only_tool" || capability.CapabilityId == "cap.admin" {
			t.Fatalf("external search added/authorized a tool: %+v", capability)
		}
	}
}

func TestListCapabilitiesSkipsExternalRankingForZDRAndUnspecifiedRetention(t *testing.T) {
	t.Parallel()

	contexts := []struct {
		name string
		zdr  *bool
	}{
		{name: "verified ZDR", zdr: func() *bool { value := true; return &value }()},
		{name: "retention unspecified", zdr: nil},
	}
	for _, test := range contexts {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			server := newTestServer()
			server.store = &capabilityStoreStub{scored: []registry.ScoredCapability{
				{Row: &registry.CapabilityRow{
					ID: "cap.track", OrgID: "org-a", Name: "Shipment lookup", Kind: models.KindTool,
					Version: "1", Description: "Read shipment state", RiskLevel: models.RiskLow, Enabled: true,
					ConfigJSON: []byte(`{"dispatch_name":"track_shipment"}`),
				}},
			}}
			searcher := &toolSearchStub{matches: []lettatools.Match{{Name: "track_shipment"}}}
			server.WithLettaToolSearcher(searcher)

			response, err := server.ListCapabilities(
				verifiedGRPCContextWithRetention(t, "org-a", test.zdr),
				&mpv1.ListCapabilitiesRequest{Query: "shipment"},
			)
			if err != nil {
				t.Fatal(err)
			}
			if searcher.calls != 0 {
				t.Fatalf("external search called %d times", searcher.calls)
			}
			if len(response.Capabilities) != 1 || response.Capabilities[0].CapabilityId != "cap.track" {
				t.Fatalf("local fallback = %+v", response.Capabilities)
			}
		})
	}
}

func TestListCapabilitiesFallsBackLocallyWhenLettaUnavailable(t *testing.T) {
	t.Parallel()

	server := newTestServer()
	server.store = &capabilityStoreStub{scored: []registry.ScoredCapability{
		{Row: &registry.CapabilityRow{
			ID: "cap.track", OrgID: "org-a", Name: "Shipment lookup", Kind: models.KindTool,
			Version: "1", Description: "Read shipment state", RiskLevel: models.RiskLow, Enabled: true,
			ConfigJSON: []byte(`{"dispatch_name":"track_shipment"}`),
		}},
	}}
	searcher := &toolSearchStub{err: errors.New("upstream unavailable")}
	server.WithLettaToolSearcher(searcher)
	nonZDR := false

	response, err := server.ListCapabilities(
		verifiedGRPCContextWithRetention(t, "org-a", &nonZDR),
		&mpv1.ListCapabilitiesRequest{Query: "shipment"},
	)
	if err != nil {
		t.Fatal(err)
	}
	if searcher.calls != 1 || len(response.Capabilities) != 1 || response.Capabilities[0].CapabilityId != "cap.track" {
		t.Fatalf("local fail-safe fallback = %+v calls=%d", response.Capabilities, searcher.calls)
	}
}

func TestCapabilityRankingReasonIsEmittedAsSafeGRPCTrailer(t *testing.T) {
	t.Parallel()

	stream := &rankingTrailerStream{}
	ctx := grpc.NewContextWithServerTransportStream(context.Background(), stream)
	recordCapabilityRanking(ctx, "local", "zdr_external_disabled")

	if got := stream.trailer.Get("x-capability-ranking-source"); len(got) != 1 || got[0] != "local" {
		t.Fatalf("ranking source trailer = %v", got)
	}
	if got := stream.trailer.Get("x-capability-ranking-reason"); len(got) != 1 || got[0] != "zdr_external_disabled" {
		t.Fatalf("ranking reason trailer = %v", got)
	}
}

func TestExactLocalToolIntersectionRejectsWildcardMalformedAndAmbiguousBindings(t *testing.T) {
	t.Parallel()

	rows := []*registry.CapabilityRow{
		{ID: "cap.first", Kind: models.KindTool, ConfigJSON: []byte(`{"dispatch_name":"duplicate_tool"}`)},
		{ID: "cap.second", Kind: models.KindTool, ConfigJSON: []byte(`{"dispatch_name":"duplicate_tool"}`)},
		{ID: "cap.wildcard", Kind: models.KindTool, ConfigJSON: []byte(`{"dispatch_name":"subagent.*"}`)},
		{ID: "cap.malformed", Kind: models.KindTool, Name: "fallback_name", ConfigJSON: []byte(`{"dispatch_name":`)},
		{ID: "cap.group", Kind: models.KindTool, ConfigJSON: []byte(`{"dispatch_name":"web_search/web_fetch"}`)},
	}
	ranked := exactLocalToolIntersection(rows, []lettatools.Match{
		{Name: "duplicate_tool"},
		{Name: "subagent.research"},
		{Name: "fallback_name"},
		{Name: "web_fetch"},
	})
	if len(ranked) != 1 || ranked[0].ID != "cap.group" {
		t.Fatalf("exact fail-closed intersection = %+v", ranked)
	}
}

func TestPromoteSkillIsQuarantinedWhenDurableStoreIsAttached(t *testing.T) {
	t.Parallel()

	server := newTestServer()
	server.store = &capabilityStoreStub{}
	ctx := verifiedGRPCContext(t, "triodelab")

	_, err := server.PromoteSkill(ctx, &mpv1.PromoteSkillRequest{
		SkillId: "cap.skill.summarize", FromScope: "agent", ToScope: "workspace",
	})
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("durable promotion status = %v", err)
	}
	capability, lookupErr := server.registry.GetForOrg("cap.skill.summarize", "", "triodelab")
	if lookupErr != nil {
		t.Fatal(lookupErr)
	}
	if capability.Scope != "agent" {
		t.Fatalf("quarantined promotion mutated memory to scope %q", capability.Scope)
	}
}

func TestEvaluatePolicyUsesDurableTenantAndFailsClosedRuntimeState(t *testing.T) {
	t.Parallel()

	now := time.Now().UTC()
	base := registry.CapabilityRow{
		ID: "cap.durable", OrgID: "org-a", Name: "Durable", Kind: models.KindTool,
		Version: "1", RiskLevel: models.RiskLow, Scope: "global", Enabled: true,
		EnabledForScopes: []string{"global"},
		RolloutState:     "stable", AvailabilityState: string(models.AvailabilityAvailable),
		ExecutionMode: models.ExecutionDirectRead, CostClass: models.CostBounded,
		HealthCheckedAt: &now,
	}

	tests := []struct {
		name         string
		mutate       func(*registry.CapabilityRow)
		wantDecision string
		wantReason   string
		wantCode     codes.Code
	}{
		{name: "current stable capability is evaluated", wantDecision: policy.DecisionAllow},
		{name: "disabled capability is denied", mutate: func(row *registry.CapabilityRow) {
			row.Enabled = false
		}, wantDecision: policy.DecisionDeny, wantReason: "capability_disabled"},
		{name: "unavailable capability is denied", mutate: func(row *registry.CapabilityRow) {
			row.AvailabilityState = string(models.AvailabilityUnavailable)
			row.ExecutionMode = models.ExecutionUnavailable
		}, wantDecision: policy.DecisionDeny, wantReason: "runtime_unavailable"},
		{name: "unhealthy capability is denied", mutate: func(row *registry.CapabilityRow) {
			row.AvailabilityState = string(models.AvailabilityUnhealthy)
			row.ExecutionMode = models.ExecutionUnavailable
		}, wantDecision: policy.DecisionDeny, wantReason: "runtime_unhealthy"},
		{name: "stale health attestation is denied", mutate: func(row *registry.CapabilityRow) {
			stale := now.Add(-models.AvailabilityAttestationTTL - time.Second)
			row.HealthCheckedAt = &stale
		}, wantDecision: policy.DecisionDeny, wantReason: "health_attestation_stale"},
		{name: "quarantined rollout is denied", mutate: func(row *registry.CapabilityRow) {
			row.RolloutState = "quarantine"
		}, wantDecision: policy.DecisionDeny, wantReason: "rollout_quarantine"},
		{name: "deprecated rollout is denied", mutate: func(row *registry.CapabilityRow) {
			row.RolloutState = "deprecated"
		}, wantDecision: policy.DecisionDeny, wantReason: "rollout_deprecated"},
		{name: "unknown risk is denied", mutate: func(row *registry.CapabilityRow) {
			row.RiskLevel = "critical-ish"
		}, wantDecision: policy.DecisionDeny, wantReason: "invalid_risk_level"},
		{name: "foreign store result is denied", mutate: func(row *registry.CapabilityRow) {
			row.OrgID = "org-b"
		}, wantCode: codes.NotFound},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			row := base
			if test.mutate != nil {
				test.mutate(&row)
			}
			store := &capabilityStoreStub{row: &row}
			server := newTestServer()
			server.store = store

			response, err := server.EvaluatePolicy(
				verifiedGRPCContext(t, "org-a"),
				&mpv1.EvaluatePolicyRequest{
					CapabilityId: row.ID, RunId: "run-a", AgentId: "agent-a", Scope: "global",
				},
			)
			if test.wantCode != codes.OK {
				if status.Code(err) != test.wantCode {
					t.Fatalf("EvaluatePolicy status = %v, want %v", status.Code(err), test.wantCode)
				}
				return
			}
			if err != nil {
				t.Fatalf("EvaluatePolicy: %v", err)
			}
			if store.gotOrg != "org-a" {
				t.Fatalf("durable lookup tenant = %q", store.gotOrg)
			}
			if response.Decision != test.wantDecision {
				t.Fatalf("decision = %+v", response)
			}
			if test.wantReason != "" && response.Reason != test.wantReason {
				t.Fatalf("reason = %q, want %q", response.Reason, test.wantReason)
			}
		})
	}
}

func TestEvaluatePolicyAllowsCurrentGlobalDispatchAttestationButRejectsStaleHealth(t *testing.T) {
	t.Parallel()

	now := time.Now().UTC()
	row := &registry.CapabilityRow{
		ID: "cap.retrieval.query", OrgID: "global", Name: "Knowledge retrieval", Kind: models.KindTool,
		Version: "1.0.0", RiskLevel: models.RiskLow, Scope: "global", Enabled: true,
		EnabledForScopes: []string{"global"}, RolloutState: "stable",
		AvailabilityState: string(models.AvailabilityAvailable), ReasonCode: "runtime_healthy",
		ExecutionMode: models.ExecutionDirectRead, CostClass: models.CostBounded, HealthCheckedAt: &now,
	}
	server := newTestServer()
	server.store = &capabilityStoreStub{row: row}
	request := &mpv1.EvaluatePolicyRequest{CapabilityId: row.ID, RunId: "run-a", AgentId: "execution-core", Scope: "global"}
	response, err := server.EvaluatePolicy(verifiedGRPCContext(t, "tenant-a"), request)
	if err != nil || response.Decision != policy.DecisionAllow {
		t.Fatalf("current global attestation response = %+v, err = %v", response, err)
	}

	stale := now.Add(-models.AvailabilityAttestationTTL - time.Second)
	row.HealthCheckedAt = &stale
	response, err = server.EvaluatePolicy(verifiedGRPCContext(t, "tenant-a"), request)
	if err != nil || response.Decision != policy.DecisionDeny || response.Reason != "health_attestation_stale" {
		t.Fatalf("stale global attestation response = %+v, err = %v", response, err)
	}
}

func TestEvaluatePolicyRejectsMissingScopeBeforeRuntimeStateEvaluation(t *testing.T) {
	t.Parallel()
	server := newTestServer()
	server.store = &capabilityStoreStub{row: &registry.CapabilityRow{
		ID: "cap.unavailable", OrgID: "org-a", Enabled: false, RiskLevel: models.RiskLow,
	}}

	_, err := server.EvaluatePolicy(
		verifiedGRPCContext(t, "org-a"),
		&mpv1.EvaluatePolicyRequest{
			CapabilityId: "cap.unavailable", RunId: "run-a", AgentId: "agent-a",
		},
	)
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("missing scope status = %v, want InvalidArgument", status.Code(err))
	}
}

func TestInMemoryGRPCMethodsRejectForeignTenantCapabilities(t *testing.T) {
	t.Parallel()

	owned := &models.Capability{
		ID: "cap.owned", Name: "Owned", Kind: models.KindTool, Version: "1", RiskLevel: models.RiskLow,
		Scope: "org", Enabled: true, OrgID: "org-a", EnabledForScopes: []string{"org"},
	}
	foreignSkill := &models.Capability{
		ID: "cap.foreign-skill", Name: "Foreign", Kind: models.KindSkill, Version: "1", RiskLevel: models.RiskLow,
		Scope: "agent", Enabled: true, OrgID: "org-b", EnabledForScopes: []string{"agent"},
	}
	global := &models.Capability{
		ID: "cap.global", Name: "Global", Kind: models.KindTool, Version: "1", RiskLevel: models.RiskLow,
		Scope: "global", Enabled: true, OrgID: "global", EnabledForScopes: []string{"global"},
	}
	reg, err := registry.NewFromSource(capabilitySourceStub{items: []*models.Capability{owned, foreignSkill, global}})
	if err != nil {
		t.Fatal(err)
	}
	server := NewServer(reg, nil, policy.New(reg))
	ctx := verifiedGRPCContext(t, "org-a")

	listed, err := server.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{})
	if err != nil {
		t.Fatalf("ListCapabilities: %v", err)
	}
	for _, capability := range listed.Capabilities {
		if capability.CapabilityId == foreignSkill.ID {
			t.Fatalf("foreign capability leaked in list: %+v", capability)
		}
	}

	for name, invoke := range map[string]func() error{
		"get": func() error {
			_, invokeErr := server.GetCapability(ctx, &mpv1.GetCapabilityRequest{CapabilityId: foreignSkill.ID})
			return invokeErr
		},
		"evaluate": func() error {
			_, invokeErr := server.EvaluatePolicy(ctx, &mpv1.EvaluatePolicyRequest{
				CapabilityId: foreignSkill.ID, RunId: "run-a", AgentId: "agent-a", Scope: "global",
			})
			return invokeErr
		},
		"validate": func() error {
			_, invokeErr := server.ValidateSkillBundle(ctx, &mpv1.ValidateSkillBundleRequest{SkillId: foreignSkill.ID})
			return invokeErr
		},
		"check promotion": func() error {
			_, invokeErr := server.CheckSkillPromotion(ctx, &mpv1.CheckSkillPromotionRequest{
				SkillId: foreignSkill.ID, FromScope: "agent", ToScope: "workspace",
			})
			return invokeErr
		},
		"promote": func() error {
			_, invokeErr := server.PromoteSkill(ctx, &mpv1.PromoteSkillRequest{
				SkillId: foreignSkill.ID, FromScope: "agent", ToScope: "workspace",
			})
			return invokeErr
		},
	} {
		t.Run(name, func(t *testing.T) {
			if code := status.Code(invoke()); code != codes.NotFound {
				t.Fatalf("foreign capability status = %v", code)
			}
		})
	}

	for capabilityID, invocationScope := range map[string]string{owned.ID: "org", global.ID: "global"} {
		if _, err := server.EvaluatePolicy(ctx, &mpv1.EvaluatePolicyRequest{
			CapabilityId: capabilityID, RunId: "run-a", AgentId: "agent-a", Scope: invocationScope,
		}); err != nil {
			t.Fatalf("authorized capability %q evaluation: %v", capabilityID, err)
		}
	}
}

func TestInMemoryGRPCMethodsRequireVerifiedIdentity(t *testing.T) {
	t.Parallel()

	server := newTestServer()
	for name, invoke := range map[string]func() error{
		"list": func() error {
			_, err := server.ListCapabilities(context.Background(), &mpv1.ListCapabilitiesRequest{})
			return err
		},
		"get": func() error {
			_, err := server.GetCapability(context.Background(), &mpv1.GetCapabilityRequest{CapabilityId: "cap.memory.search"})
			return err
		},
		"evaluate": func() error {
			_, err := server.EvaluatePolicy(context.Background(), &mpv1.EvaluatePolicyRequest{
				CapabilityId: "cap.memory.search", RunId: "run-a", AgentId: "agent-a", Scope: "workspace",
			})
			return err
		},
		"validate": func() error {
			_, err := server.ValidateSkillBundle(context.Background(), &mpv1.ValidateSkillBundleRequest{SkillId: "cap.skill.summarize"})
			return err
		},
		"check promotion": func() error {
			_, err := server.CheckSkillPromotion(context.Background(), &mpv1.CheckSkillPromotionRequest{
				SkillId: "cap.skill.summarize", FromScope: "agent", ToScope: "workspace",
			})
			return err
		},
		"promote": func() error {
			_, err := server.PromoteSkill(context.Background(), &mpv1.PromoteSkillRequest{
				SkillId: "cap.skill.summarize", FromScope: "agent", ToScope: "workspace",
			})
			return err
		},
	} {
		t.Run(name, func(t *testing.T) {
			if code := status.Code(invoke()); code != codes.Unauthenticated {
				t.Fatalf("missing verified identity status = %v", code)
			}
		})
	}
}

func newTestServer() *Server {
	reg := registry.NewRegistry()
	pol := policy.New(reg)
	return NewServer(reg, nil, pol)
}

func newAvailablePolicyTestServer(t *testing.T) *Server {
	t.Helper()
	checkedAt := time.Now().UTC()
	available := func(id, name, kind, version, risk string) *models.Capability {
		executionMode := models.ExecutionDirectRead
		if risk == models.RiskHigh {
			executionMode = models.ExecutionAgentic
		}
		return &models.Capability{
			ID: id, Name: name, Kind: kind, Version: version, RiskLevel: risk,
			Scope: "global", Enabled: true, OrgID: "triodelab",
			EnabledForScopes:  []string{"global"},
			AvailabilityState: string(models.AvailabilityAvailable),
			ExecutionMode:     executionMode, CostClass: models.CostBounded,
			HealthCheckedAt: &checkedAt,
		}
	}
	capabilities := []*models.Capability{
		available("cap.memory.search", "Search Memory", models.KindMemory, "1.1.0", models.RiskLow),
		available("cap.tool.http", "HTTP Fetch", models.KindTool, "1.0.0", models.RiskMedium),
		available("cap.sandbox.exec", "Execute In Sandbox", models.KindSandbox, "1.0.0", models.RiskHigh),
	}
	reg, err := registry.NewFromSource(capabilitySourceStub{items: capabilities})
	if err != nil {
		t.Fatal(err)
	}
	return NewServer(reg, nil, policy.New(reg))
}

func TestRegisterWiresServiceDescriptor(t *testing.T) {
	g := grpc.NewServer()
	// Register must not panic and the service must be advertised.
	Register(g, newTestServer())
	info := g.GetServiceInfo()
	if _, ok := info["model_plane.v1.CapabilityCore"]; !ok {
		t.Fatalf("expected service model_plane.v1.CapabilityCore to be registered, got %v", info)
	}
}

func TestListCapabilities(t *testing.T) {
	s := newTestServer()
	ctx := verifiedGRPCContext(t, "triodelab")

	t.Run("no filter returns seeded set", func(t *testing.T) {
		resp, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if len(resp.Capabilities) == 0 {
			t.Fatalf("expected seeded capabilities, got none")
		}
		for _, capability := range resp.Capabilities {
			if capability.State != "unavailable" {
				t.Fatalf("unattested capability %q state = %q", capability.CapabilityId, capability.State)
			}
			if capability.ReasonCode != "health_not_attested" {
				t.Fatalf("unattested capability %q reason = %q", capability.CapabilityId, capability.ReasonCode)
			}
			if capability.ExecutionMode != "unavailable" {
				t.Fatalf("unattested capability %q execution mode = %q", capability.CapabilityId, capability.ExecutionMode)
			}
		}
	})

	t.Run("kind filter narrows results", func(t *testing.T) {
		resp, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{KindFilter: "tool"})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		for _, c := range resp.Capabilities {
			if c.Kind != "tool" {
				t.Errorf("expected kind=tool, got %q for %s", c.Kind, c.CapabilityId)
			}
		}
		if len(resp.Capabilities) == 0 {
			t.Fatalf("expected at least one tool capability")
		}
	})

	t.Run("query matches name substring", func(t *testing.T) {
		resp, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{Query: "memory"})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if len(resp.Capabilities) == 0 {
			t.Fatalf("expected memory capabilities, got none")
		}
	})

	t.Run("pagination via limit+afterID", func(t *testing.T) {
		first, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{Limit: 1})
		if err != nil {
			t.Fatalf("first page: %v", err)
		}
		if len(first.Capabilities) != 1 {
			t.Fatalf("expected 1 item, got %d", len(first.Capabilities))
		}
		if !first.HasMore {
			t.Fatalf("expected HasMore=true on first page")
		}
		second, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{Limit: 1, AfterId: first.Capabilities[0].CapabilityId})
		if err != nil {
			t.Fatalf("second page: %v", err)
		}
		if len(second.Capabilities) != 1 {
			t.Fatalf("expected 1 item on page 2, got %d", len(second.Capabilities))
		}
		if second.Capabilities[0].CapabilityId == first.Capabilities[0].CapabilityId {
			t.Errorf("page 2 returned same item as page 1: %s", first.Capabilities[0].CapabilityId)
		}
	})

	t.Run("unknown kind yields empty result", func(t *testing.T) {
		resp, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{KindFilter: "does-not-exist"})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if len(resp.Capabilities) != 0 {
			t.Errorf("expected empty, got %d", len(resp.Capabilities))
		}
		if resp.HasMore {
			t.Errorf("expected HasMore=false on empty result")
		}
	})
}

func TestGetCapability(t *testing.T) {
	s := newTestServer()
	ctx := verifiedGRPCContext(t, "triodelab")

	t.Run("found", func(t *testing.T) {
		detail, err := s.GetCapability(ctx, &mpv1.GetCapabilityRequest{CapabilityId: "cap.memory.search"})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if detail.CapabilityId != "cap.memory.search" {
			t.Errorf("got %s", detail.CapabilityId)
		}
	})

	t.Run("empty id -> InvalidArgument", func(t *testing.T) {
		_, err := s.GetCapability(ctx, &mpv1.GetCapabilityRequest{})
		if status.Code(err) != codes.InvalidArgument {
			t.Errorf("expected InvalidArgument, got %v", err)
		}
	})

	t.Run("unknown id -> NotFound", func(t *testing.T) {
		_, err := s.GetCapability(ctx, &mpv1.GetCapabilityRequest{CapabilityId: "cap.does.not.exist"})
		if status.Code(err) != codes.NotFound {
			t.Errorf("expected NotFound, got %v", err)
		}
	})

	t.Run("version mismatch -> FailedPrecondition", func(t *testing.T) {
		_, err := s.GetCapability(ctx, &mpv1.GetCapabilityRequest{CapabilityId: "cap.memory.search", VersionConstraint: "9.9.9"})
		if status.Code(err) != codes.FailedPrecondition {
			t.Errorf("expected FailedPrecondition, got %v", err)
		}
	})
}

func TestEvaluatePolicy(t *testing.T) {
	s := newAvailablePolicyTestServer(t)
	ctx := verifiedGRPCContext(t, "triodelab")

	baseReq := func(capID string) *mpv1.EvaluatePolicyRequest {
		return &mpv1.EvaluatePolicyRequest{
			CapabilityId: capID,
			RunId:        "run-1",
			AgentId:      "agent-1",
			OrgId:        "org-1",
			Scope:        "global",
		}
	}

	t.Run("low-risk -> allow", func(t *testing.T) {
		resp, err := s.EvaluatePolicy(ctx, baseReq("cap.memory.search"))
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if resp.Decision != policy.DecisionAllow {
			t.Errorf("expected allow, got %s", resp.Decision)
		}
		if resp.BudgetContext == "" {
			t.Errorf("expected non-empty budget context")
		}
	})

	t.Run("medium-risk -> allow with constrained budget", func(t *testing.T) {
		resp, err := s.EvaluatePolicy(ctx, baseReq("cap.tool.http"))
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if resp.Decision != policy.DecisionAllow {
			t.Errorf("expected allow, got %s", resp.Decision)
		}
		if resp.BudgetContext == "" {
			t.Errorf("expected constrained budget context")
		}
	})

	t.Run("high-risk -> ask", func(t *testing.T) {
		resp, err := s.EvaluatePolicy(ctx, baseReq("cap.sandbox.exec"))
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if resp.Decision != policy.DecisionAsk {
			t.Errorf("expected ask, got %s", resp.Decision)
		}
		if resp.Reason == "" {
			t.Errorf("expected non-empty reason for deny")
		}
	})

	t.Run("missing caller org is replaced by verified tenant", func(t *testing.T) {
		req := baseReq("cap.memory.search")
		req.OrgId = ""
		response, err := s.EvaluatePolicy(ctx, req)
		if err != nil || response.Decision != policy.DecisionAllow {
			t.Errorf("expected tenant-derived allow, got response=%+v error=%v", response, err)
		}
	})

	t.Run("missing or unsupported scope is rejected", func(t *testing.T) {
		for _, scope := range []string{"", "something-random", "agent", "run", "thread", "workspace", "user"} {
			req := baseReq("cap.memory.search")
			req.Scope = scope
			_, err := s.EvaluatePolicy(ctx, req)
			if status.Code(err) != codes.InvalidArgument {
				t.Fatalf("scope %q status = %v, want InvalidArgument", scope, status.Code(err))
			}
		}
	})

	t.Run("unknown capability -> NotFound", func(t *testing.T) {
		_, err := s.EvaluatePolicy(ctx, baseReq("cap.does.not.exist"))
		if status.Code(err) != codes.NotFound {
			t.Errorf("expected NotFound, got %v", err)
		}
	})
}

func TestSkillPromotionRPCs(t *testing.T) {
	s := newTestServer()
	ctx := verifiedGRPCContext(t, "triodelab")

	validation, err := s.ValidateSkillBundle(ctx, &mpv1.ValidateSkillBundleRequest{SkillId: "cap.skill.summarize"})
	if err != nil {
		t.Fatalf("ValidateSkillBundle: %v", err)
	}
	if !validation.Valid {
		t.Fatalf("expected skill to validate, got errors %v", validation.Errors)
	}

	gate, err := s.CheckSkillPromotion(ctx, &mpv1.CheckSkillPromotionRequest{
		SkillId:   "cap.skill.summarize",
		FromScope: "agent",
		ToScope:   "workspace",
	})
	if err != nil {
		t.Fatalf("CheckSkillPromotion: %v", err)
	}
	if !gate.Passed {
		t.Fatalf("expected promotion gate to pass, got %+v", gate)
	}

	promoted, err := s.PromoteSkill(ctx, &mpv1.PromoteSkillRequest{
		SkillId:   "cap.skill.summarize",
		FromScope: "agent",
		ToScope:   "workspace",
	})
	if err != nil {
		t.Fatalf("PromoteSkill: %v", err)
	}
	if !promoted.Promoted {
		t.Fatalf("expected promotion success, got %+v", promoted)
	}
	if promoted.Capability == nil || promoted.Capability.Scope != "workspace" {
		t.Fatalf("expected promoted scope workspace, got %+v", promoted.Capability)
	}

	failedGate, err := s.CheckSkillPromotion(ctx, &mpv1.CheckSkillPromotionRequest{
		SkillId:   "cap.skill.summarize",
		FromScope: "agent",
		ToScope:   "workspace",
	})
	if err != nil {
		t.Fatalf("CheckSkillPromotion mismatch: %v", err)
	}
	if failedGate.Passed {
		t.Fatalf("expected gate mismatch to fail after promotion, got %+v", failedGate)
	}
}
