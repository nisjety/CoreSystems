package handlers

import (
	"context"
	"errors"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/nats"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	pb "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/proto/user/v1"
)

// DocumentAclHandler implements pb.DocumentAccessServiceServer.
type DocumentAclHandler struct {
	pb.UnimplementedDocumentAccessServiceServer
	aclRepo         *users.AclRepository
	publisher       *nats.Publisher
	sharedPublisher *nats.SharedPublisher
}

// NewDocumentAclHandler constructs a DocumentAclHandler.
func NewDocumentAclHandler(
	aclRepo *users.AclRepository,
	publisher *nats.Publisher,
	sharedPublisher *nats.SharedPublisher,
) *DocumentAclHandler {
	return &DocumentAclHandler{
		aclRepo:         aclRepo,
		publisher:       publisher,
		sharedPublisher: sharedPublisher,
	}
}

// GrantDocumentAccess creates an ACL entry granting a user access to a document.
func (h *DocumentAclHandler) GrantDocumentAccess(
	ctx context.Context, req *pb.GrantDocumentAccessRequest,
) (*pb.GrantDocumentAccessResponse, error) {
	// 🔔 entry
	acl := users.DocumentAcl{
		AclID:           uuid.New().String(),
		OrgID:           req.OrgId,
		DocumentID:      req.DocumentId,
		UserID:          req.UserId,
		PermissionLevel: req.PermissionLevel,
	}

	if err := h.aclRepo.Create(ctx, &acl); err != nil {
		// ❌ fatal
		return nil, status.Errorf(codes.Internal, "failed to create acl: %v", err)
	}

	if h.sharedPublisher != nil {
		h.sharedPublisher.PublishDocumentAclChanged(
			ctx, acl.AclID, acl.OrgID, acl.DocumentID, acl.UserID, acl.PermissionLevel, "grant",
		)
	}
	_ = h.publisher.PublishActivityLogged(ctx, req.UserId, "grant_document_access", req.DocumentId, "", "", nil)

	// ✅ success
	return &pb.GrantDocumentAccessResponse{Acl: &pb.DocumentAclEntry{
		AclId:           acl.AclID,
		OrgId:           acl.OrgID,
		DocumentId:      acl.DocumentID,
		UserId:          acl.UserID,
		PermissionLevel: acl.PermissionLevel,
	}}, nil
}

// RevokeDocumentAccess removes an ACL entry for a user-document pair.
func (h *DocumentAclHandler) RevokeDocumentAccess(
	ctx context.Context, req *pb.RevokeDocumentAccessRequest,
) (*pb.RevokeDocumentAccessResponse, error) {
	// 🔔 entry
	if err := h.aclRepo.DeleteByUser(ctx, req.OrgId, req.DocumentId, req.UserId); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, status.Errorf(codes.NotFound, "acl entry not found")
		}
		// ❌ fatal
		return nil, status.Errorf(codes.Internal, "failed to revoke acl: %v", err)
	}

	if h.sharedPublisher != nil {
		h.sharedPublisher.PublishDocumentAclChanged(ctx, "", req.OrgId, req.DocumentId, req.UserId, "", "revoke")
	}
	_ = h.publisher.PublishActivityLogged(ctx, req.UserId, "revoke_document_access", req.DocumentId, "", "", nil)

	// ✅ success
	return &pb.RevokeDocumentAccessResponse{Revoked: true}, nil
}

// GetDocumentAccess retrieves the ACL entry for a user-document pair.
func (h *DocumentAclHandler) GetDocumentAccess(
	ctx context.Context, req *pb.GetDocumentAccessRequest,
) (*pb.GetDocumentAccessResponse, error) {
	// 🔔 entry
	acl, err := h.aclRepo.GetByUser(ctx, req.OrgId, req.DocumentId, req.UserId)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, status.Errorf(codes.NotFound, "acl entry not found")
		}
		// ❌ fatal
		return nil, status.Errorf(codes.Internal, "failed to get acl: %v", err)
	}

	// ✅ success
	return &pb.GetDocumentAccessResponse{
		Acl: &pb.DocumentAclEntry{
			AclId:           acl.AclID,
			OrgId:           acl.OrgID,
			DocumentId:      acl.DocumentID,
			UserId:          acl.UserID,
			PermissionLevel: acl.PermissionLevel,
		},
	}, nil
}

// ListDocumentAccess returns all ACL entries for a document within an org.
func (h *DocumentAclHandler) ListDocumentAccess(
	ctx context.Context, req *pb.ListDocumentAccessRequest,
) (*pb.ListDocumentAccessResponse, error) {
	// 🔔 entry
	acls, err := h.aclRepo.ListByDocument(ctx, req.OrgId, req.DocumentId)
	if err != nil {
		// ❌ fatal
		return nil, status.Errorf(codes.Internal, "failed to list acls: %v", err)
	}

	entries := make([]*pb.DocumentAclEntry, 0, len(acls))
	for _, a := range acls {
		entries = append(entries, &pb.DocumentAclEntry{
			AclId:           a.AclID,
			OrgId:           a.OrgID,
			DocumentId:      a.DocumentID,
			UserId:          a.UserID,
			PermissionLevel: a.PermissionLevel,
		})
	}

	// ✅ success
	return &pb.ListDocumentAccessResponse{Acls: entries}, nil
}

// CheckDocumentAccess returns whether a user has any ACL entry for a document.
func (h *DocumentAclHandler) CheckDocumentAccess(
	ctx context.Context, req *pb.CheckDocumentAccessRequest,
) (*pb.CheckDocumentAccessResponse, error) {
	// 🔔 entry
	ok, err := h.aclRepo.HasAccess(ctx, req.OrgId, req.DocumentId, req.UserId)
	if err != nil {
		// ❌ fatal
		return nil, status.Errorf(codes.Internal, "failed to check acl: %v", err)
	}

	// ✅ success
	return &pb.CheckDocumentAccessResponse{Allowed: ok}, nil
}
