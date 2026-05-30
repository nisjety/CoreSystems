package feed

import (
	"context"
	"errors"
	"strings"
	"time"
)

type Service struct {
	repo *PGRepository
	now  func() time.Time
}

func NewService(repo *PGRepository) *Service {
	return &Service{repo: repo, now: time.Now}
}

func (s *Service) Create(ctx context.Context, params CreateParams) (*Notification, error) {
	if s == nil {
		return nil, errors.New("feed service not configured")
	}
	return s.repo.Create(ctx, params)
}

func (s *Service) List(ctx context.Context, params ListParams) (*Feed, error) {
	if s == nil {
		return nil, errors.New("feed service not configured")
	}
	if strings.TrimSpace(params.RecipientID) == "" {
		return nil, errors.New("feed list: recipient_id required")
	}
	return s.repo.List(ctx, params)
}

func (s *Service) UnreadCount(ctx context.Context, recipientID string) (int, error) {
	if s == nil {
		return 0, errors.New("feed service not configured")
	}
	return s.repo.UnreadCount(ctx, recipientID)
}

func (s *Service) UnseenCount(ctx context.Context, recipientID string) (int, error) {
	if s == nil {
		return 0, errors.New("feed service not configured")
	}
	return s.repo.UnseenCount(ctx, recipientID)
}

func (s *Service) MarkRead(ctx context.Context, recipientID, id string) (*Notification, error) {
	if s == nil {
		return nil, errors.New("feed service not configured")
	}
	return s.repo.MarkRead(ctx, recipientID, id, s.now())
}

func (s *Service) MarkAllRead(ctx context.Context, recipientID string) (int, error) {
	if s == nil {
		return 0, errors.New("feed service not configured")
	}
	return s.repo.MarkAllRead(ctx, recipientID, s.now())
}

func (s *Service) MarkAllSeen(ctx context.Context, recipientID string) (int, error) {
	if s == nil {
		return 0, errors.New("feed service not configured")
	}
	return s.repo.MarkAllSeen(ctx, recipientID, s.now())
}

func (s *Service) Archive(ctx context.Context, recipientID, id string) error {
	if s == nil {
		return errors.New("feed service not configured")
	}
	return s.repo.Archive(ctx, recipientID, id, s.now())
}
