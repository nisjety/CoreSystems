package workspace

import (
	"context"
	"crypto/sha1"
	"encoding/hex"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/eventing"
)

type Service struct {
	repository *Repository
	publisher  *eventing.Publisher
}

func NewService(repository *Repository, publisher *eventing.Publisher) *Service {
	return &Service{repository: repository, publisher: publisher}
}

func (s *Service) ResolveWorkspace(ctx context.Context, orgID string, userID string) (Binding, error) {
	cleanOrgID := strings.TrimSpace(orgID)
	cleanUserID := strings.TrimSpace(userID)
	if cleanOrgID == "" {
		return Binding{
			OrgID:       "",
			WorkspaceID: buildPersonalWorkspaceID(cleanUserID),
			CreatedBy:   cleanUserID,
		}, nil
	}

	binding, err := s.repository.GetBinding(ctx, cleanOrgID)
	if err == nil {
		return binding, nil
	}
	if !IsNotFound(err) {
		return Binding{}, err
	}

	binding, err = s.repository.UpsertBinding(ctx, Binding{
		OrgID:       cleanOrgID,
		WorkspaceID: buildWorkspaceID(cleanOrgID),
		CreatedBy:   cleanUserID,
	})
	if err != nil {
		return Binding{}, err
	}

	_ = s.publisher.Publish(ctx, "aqencia.application.affine.workspace.bound", map[string]any{
		"org_id":       binding.OrgID,
		"workspace_id": binding.WorkspaceID,
		"user_id":      binding.CreatedBy,
		"occurred_at":  time.Now().UTC(),
	})

	return binding, nil
}

func buildWorkspaceID(orgID string) string {
	hash := sha1.Sum([]byte(orgID))
	return "planner-" + hex.EncodeToString(hash[:])[:12]
}

func buildPersonalWorkspaceID(userID string) string {
	cleanUserID := strings.TrimSpace(userID)
	if cleanUserID == "" {
		return "planner-anonymous"
	}

	hash := sha1.Sum([]byte(cleanUserID))
	return "planner-user-" + hex.EncodeToString(hash[:])[:12]
}
