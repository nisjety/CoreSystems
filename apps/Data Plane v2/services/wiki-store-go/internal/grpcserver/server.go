// Package grpcserver implements `wiki.v1.WikiService` on top of WikiRepo.
//
// Mirrors the chi HTTP handler surface in `internal/handler/wiki.go` 1:1 —
// both wires call the same repo, so the gRPC server is a thin mapping layer.
// The Model Plane gateway expects this on :50054 (DATAPLANE_WIKI_ADDR).
package grpcserver

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/jackc/pgx/v5"
	wikipb "github.com/triodelab/dataplane/gen/go/wiki/v1"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/structpb"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/model"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/repo"
)

// WikiServer satisfies wikipb.WikiServiceServer.
type WikiServer struct {
	wikipb.UnimplementedWikiServiceServer
	repo *repo.WikiRepo
}

func New(r *repo.WikiRepo) *WikiServer {
	return &WikiServer{repo: r}
}

// ── Model → proto mappers ────────────────────────────────────────────────

func pageToPB(p *model.WikiPage) *wikipb.WikiPage {
	if p == nil {
		return nil
	}
	current := ""
	if p.CurrentVersionID != nil {
		current = *p.CurrentVersionID
	}
	var backlinks []string
	if len(p.Backlinks) > 0 {
		// Best-effort decode: model stores as JSON array of page_id strings.
		_ = json.Unmarshal(p.Backlinks, &backlinks)
	}
	return &wikipb.WikiPage{
		PageId:           p.PageID,
		OrgId:            p.OrgID,
		WorkspaceId:      p.WorkspaceID,
		Title:            p.Title,
		Path:             p.Path,
		CurrentVersionId: current,
		Status:           p.Status,
		Backlinks:        backlinks,
		CreatedAt:        timestamppb.New(p.CreatedAt),
		UpdatedAt:        timestamppb.New(p.UpdatedAt),
		Metadata:         rawToStruct(p.Metadata),
	}
}

func versionToPB(v *model.WikiPageVersion) *wikipb.WikiPageVersion {
	if v == nil {
		return nil
	}
	out := &wikipb.WikiPageVersion{
		VersionId:  v.VersionID,
		PageId:     v.PageID,
		EditReason: strPtrOr(v.EditReason, ""),
		Status:     v.VersionStatus,
		CreatedAt:  timestamppb.New(v.CreatedAt),
		Metadata:   rawToStruct(v.Metadata),
	}
	if v.Content != nil {
		out.Content = *v.Content
	}
	if v.SourceRefs != nil {
		_ = json.Unmarshal(v.SourceRefs, &out.SourceRefs)
	}
	out.ProposedByAgent = v.ProposedByAgent
	out.ProposedByUser = v.ProposedByUser
	out.ApprovedBy = v.ApprovedBy
	if v.PublishedAt != nil {
		out.PublishedAt = timestamppb.New(*v.PublishedAt)
	}
	if v.SafeHTML != nil {
		out.SafeHtml = v.SafeHTML
	}
	out.SafeHtmlOk = v.SafeHTMLOK
	return out
}

func proposalToPB(p *model.WikiProposal) *wikipb.WikiProposal {
	if p == nil {
		return nil
	}
	editReason := ""
	if p.EditReason != nil {
		editReason = *p.EditReason
	}
	proposedByAgent := ""
	if p.ProposedByAgent != nil {
		proposedByAgent = *p.ProposedByAgent
	}
	var sourceRefs []string
	if len(p.SourceRefs) > 0 {
		_ = json.Unmarshal(p.SourceRefs, &sourceRefs)
	}
	return &wikipb.WikiProposal{
		ProposalId:      p.ProposalID,
		PageId:          p.PageID,
		OrgId:           p.OrgID,
		ProposedContent: p.ProposedContent,
		EditReason:      editReason,
		ProposedByAgent: proposedByAgent,
		SourceRefs:      sourceRefs,
		Status:          p.ProposalStatus,
		CreatedAt:       timestamppb.New(p.CreatedAt),
		Metadata:        rawToStruct(p.Metadata),
	}
}

func rawToStruct(raw json.RawMessage) *structpb.Struct {
	if len(raw) == 0 {
		return nil
	}
	var m map[string]interface{}
	if err := json.Unmarshal(raw, &m); err != nil {
		return nil
	}
	s, err := structpb.NewStruct(m)
	if err != nil {
		return nil
	}
	return s
}

func strPtrOr(p *string, fallback string) string {
	if p == nil {
		return fallback
	}
	return *p
}

func notFoundOrInternal(err error, what string) error {
	if errors.Is(err, pgx.ErrNoRows) {
		return status.Errorf(codes.NotFound, "%s not found", what)
	}
	return status.Errorf(codes.Internal, "%s unavailable", what)
}

// ── Service implementation ───────────────────────────────────────────────

func (s *WikiServer) GetPage(ctx context.Context, req *wikipb.GetPageRequest) (*wikipb.GetPageResponse, error) {
	orgID, err := authctx.TenantID(ctx, req.GetOrgId())
	if err != nil {
		return nil, err
	}
	if req.PageId == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id and page_id are required")
	}
	page, err := s.repo.GetPage(ctx, orgID, req.PageId)
	if err != nil {
		return nil, notFoundOrInternal(err, "page")
	}
	var version *model.WikiPageVersion
	if req.VersionId != nil && *req.VersionId != "" {
		version, _ = s.repo.GetVersionForPage(ctx, orgID, req.PageId, *req.VersionId)
	} else if page.CurrentVersionID != nil {
		version, _ = s.repo.GetVersionForPage(ctx, orgID, req.PageId, *page.CurrentVersionID)
	}
	return &wikipb.GetPageResponse{Page: pageToPB(page), Version: versionToPB(version)}, nil
}

func (s *WikiServer) GetPageByPath(ctx context.Context, req *wikipb.GetPageByPathRequest) (*wikipb.GetPageByPathResponse, error) {
	orgID, err := authctx.TenantID(ctx, req.GetOrgId())
	if err != nil {
		return nil, err
	}
	if req.Path == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id and path are required")
	}
	page, err := s.repo.GetPageByPath(ctx, orgID, req.Path)
	if err != nil {
		return nil, notFoundOrInternal(err, "page")
	}
	var version *model.WikiPageVersion
	if req.VersionId != nil && *req.VersionId != "" {
		version, _ = s.repo.GetVersionForPage(ctx, orgID, page.PageID, *req.VersionId)
	} else if page.CurrentVersionID != nil {
		version, _ = s.repo.GetVersionForPage(ctx, orgID, page.PageID, *page.CurrentVersionID)
	}
	return &wikipb.GetPageByPathResponse{Page: pageToPB(page), Version: versionToPB(version)}, nil
}

func (s *WikiServer) ListPageVersions(ctx context.Context, req *wikipb.ListPageVersionsRequest) (*wikipb.ListPageVersionsResponse, error) {
	orgID, authErr := authctx.TenantID(ctx, req.GetOrgId())
	if authErr != nil {
		return nil, authErr
	}
	limit := int(req.Limit)
	if limit <= 0 {
		limit = 20
	}
	rows, total, err := s.repo.ListVersions(ctx, orgID, req.PageId, limit, int(req.Offset))
	if err != nil {
		return nil, status.Error(codes.Internal, "versions unavailable")
	}
	versions := make([]*wikipb.WikiPageVersion, 0, len(rows))
	for i := range rows {
		versions = append(versions, versionToPB(&rows[i]))
	}
	return &wikipb.ListPageVersionsResponse{Versions: versions, Total: int32(total)}, nil
}

func (s *WikiServer) GetPageSources(ctx context.Context, req *wikipb.GetPageSourcesRequest) (*wikipb.GetPageSourcesResponse, error) {
	orgID, authErr := authctx.TenantID(ctx, req.GetOrgId())
	if authErr != nil {
		return nil, authErr
	}
	// Proto's WikiSourceLog shape doesn't match the repo's `model.SourceLog`
	// (proto carries original_chunks/processing_model/synthesis_prompt_hash;
	// model carries source_type/source_ref/sync_status). Return the most
	// recent source log mapped on best-effort — see WIRE_SURFACE_PLAN reconcile
	// section. Caller relying on richer fields should consult the HTTP API
	// until the model + proto reconcile.
	logs, _, err := s.repo.ListSourceLogs(ctx, orgID, req.PageId, 1, 0)
	if err != nil {
		return nil, status.Error(codes.Internal, "source logs unavailable")
	}
	if len(logs) == 0 {
		return &wikipb.GetPageSourcesResponse{}, nil
	}
	l := logs[0]
	return &wikipb.GetPageSourcesResponse{
		SourceLog: &wikipb.WikiSourceLog{
			LogId:     l.LogID,
			PageId:    l.PageID,
			CreatedAt: timestamppb.New(l.CreatedAt),
			Metadata:  rawToStruct(l.Details),
		},
	}, nil
}

func (s *WikiServer) ListMaintenanceIssues(ctx context.Context, req *wikipb.ListMaintenanceIssuesRequest) (*wikipb.ListMaintenanceIssuesResponse, error) {
	orgID, authErr := authctx.TenantID(ctx, req.GetOrgId())
	if authErr != nil {
		return nil, authErr
	}
	// Repo doesn't filter by status; client-side filter for parity with HTTP.
	pageID := ""
	if req.PageId != nil {
		pageID = *req.PageId
	}
	limit := int(req.Limit)
	if limit <= 0 {
		limit = 50
	}
	rows, total, err := s.repo.ListMaintenanceLogs(ctx, orgID, pageID, limit, int(req.Offset))
	if err != nil {
		return nil, status.Error(codes.Internal, "maintenance logs unavailable")
	}
	issues := make([]*wikipb.WikiMaintenanceLog, 0, len(rows))
	for _, r := range rows {
		if req.Status != "" && req.Status != r.Action {
			continue
		}
		// Proto/model drift: model has action+actor; proto has issue_type+status.
		// We map action→issue_type so the field carries SOMETHING meaningful;
		// "status" is left empty until model gains a status column.
		issues = append(issues, &wikipb.WikiMaintenanceLog{
			LogId:     r.LogID,
			PageId:    r.PageID,
			IssueType: r.Action,
			Details:   rawToStruct(r.Details),
			CreatedAt: timestamppb.New(r.CreatedAt),
		})
	}
	return &wikipb.ListMaintenanceIssuesResponse{Issues: issues, Total: int32(total)}, nil
}

func (s *WikiServer) GetBacklinks(ctx context.Context, req *wikipb.GetBacklinksRequest) (*wikipb.GetBacklinksResponse, error) {
	orgID, authErr := authctx.TenantID(ctx, req.GetOrgId())
	if authErr != nil {
		return nil, authErr
	}
	rows, err := s.repo.GetBacklinks(ctx, orgID, req.PageId)
	if err != nil {
		return nil, status.Error(codes.Internal, "backlinks unavailable")
	}
	pages := make([]*wikipb.WikiPage, 0, len(rows))
	for i := range rows {
		pages = append(pages, pageToPB(&rows[i]))
	}
	return &wikipb.GetBacklinksResponse{Pages: pages}, nil
}

func (s *WikiServer) CreatePage(ctx context.Context, req *wikipb.CreatePageRequest) (*wikipb.CreatePageResponse, error) {
	orgID, authErr := authctx.TenantID(ctx, req.GetOrgId())
	if authErr != nil {
		return nil, authErr
	}
	if req.WorkspaceId == "" || req.Title == "" || req.Path == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id, workspace_id, title and path are required")
	}
	page, version, err := s.repo.CreatePage(ctx, model.CreatePageInput{
		OrgID:          orgID,
		WorkspaceID:    req.WorkspaceId,
		Title:          req.Title,
		Path:           req.Path,
		InitialContent: req.InitialContent,
	})
	if err != nil {
		return nil, status.Error(codes.Internal, "failed to create page")
	}
	return &wikipb.CreatePageResponse{Page: pageToPB(page), Version: versionToPB(version)}, nil
}

func (s *WikiServer) UpdatePageVersion(ctx context.Context, req *wikipb.UpdatePageVersionRequest) (*wikipb.UpdatePageVersionResponse, error) {
	orgID, authErr := authctx.TenantID(ctx, req.GetOrgId())
	if authErr != nil {
		return nil, authErr
	}
	if req.PageId == "" || req.NewContent == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id, page_id and new_content are required")
	}
	claims, _ := authctx.FromContext(ctx)
	proposedBy := claims.PrincipalID()
	version, err := s.repo.CreateVersion(ctx, model.UpdateVersionInput{
		PageID:     req.PageId,
		OrgID:      orgID,
		NewContent: req.NewContent,
		EditReason: req.EditReason,
		ProposedBy: &proposedBy,
	})
	if err != nil {
		return nil, status.Error(codes.Internal, "failed to update version")
	}
	return &wikipb.UpdatePageVersionResponse{NewVersion: versionToPB(version)}, nil
}

func (s *WikiServer) SubmitProposal(ctx context.Context, req *wikipb.SubmitProposalRequest) (*wikipb.SubmitProposalResponse, error) {
	orgID, authErr := authctx.TenantID(ctx, req.GetOrgId())
	if authErr != nil {
		return nil, authErr
	}
	claims, _ := authctx.FromContext(ctx)
	proposal, err := s.repo.SubmitProposal(ctx, model.SubmitProposalInput{
		PageID:          req.PageId,
		OrgID:           orgID,
		ProposedContent: req.ProposedContent,
		EditReason:      req.EditReason,
		ProposedByAgent: claims.PrincipalID(),
		SourceRefs:      req.SourceRefs,
	})
	if err != nil {
		return nil, status.Error(codes.Internal, "failed to submit proposal")
	}
	return &wikipb.SubmitProposalResponse{Proposal: proposalToPB(proposal)}, nil
}

func (s *WikiServer) ReviewProposal(ctx context.Context, req *wikipb.ReviewProposalRequest) (*wikipb.ReviewProposalResponse, error) {
	orgID, authErr := authctx.TenantID(ctx, req.GetOrgId())
	if authErr != nil {
		return nil, authErr
	}
	if req.Decision != "accept" && req.Decision != "reject" {
		return nil, status.Error(codes.InvalidArgument, "decision must be 'accept' or 'reject'")
	}
	claims, _ := authctx.FromContext(ctx)
	proposal, version, err := s.repo.ReviewProposal(ctx, model.ReviewProposalInput{
		ProposalID: req.ProposalId,
		OrgID:      orgID,
		Decision:   req.Decision,
		ReviewedBy: claims.PrincipalID(),
	})
	if err != nil {
		return nil, status.Error(codes.Internal, "failed to review proposal")
	}
	return &wikipb.ReviewProposalResponse{
		Proposal:   proposalToPB(proposal),
		NewVersion: versionToPB(version),
	}, nil
}
