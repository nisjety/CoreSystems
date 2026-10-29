package grpc

import (
	"context"
	"fmt"
	"time"

	whitelistv1 "github.com/coresystem/integration-gateway/api/proto/whitelist/v1"
	"github.com/coresystem/integration-gateway/internal/whitelist"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// WhitelistServiceServer implements the gRPC WhitelistService
type WhitelistServiceServer struct {
	whitelistv1.UnimplementedWhitelistServiceServer
	manager *whitelist.Manager
}

// NewWhitelistServiceServer creates a new gRPC whitelist service server
func NewWhitelistServiceServer(manager *whitelist.Manager) *WhitelistServiceServer {
	return &WhitelistServiceServer{
		manager: manager,
	}
}

// AddToWhitelist adds an IP/CIDR to the whitelist
func (s *WhitelistServiceServer) AddToWhitelist(ctx context.Context, req *whitelistv1.AddToWhitelistRequest) (*whitelistv1.AddToWhitelistResponse, error) {
	var expiresAt *timestamppb.Timestamp = req.ExpiresAt
	var expiry *time.Time
	if expiresAt != nil {
		t := expiresAt.AsTime()
		expiry = &t
	}

	err := s.manager.AddToWhitelist(req.Ip, req.Description, req.AddedBy, expiry)
	if err != nil {
		return &whitelistv1.AddToWhitelistResponse{
			Success: false,
			Message: err.Error(),
		}, nil
	}

	return &whitelistv1.AddToWhitelistResponse{
		Success: true,
		Message: "IP added to whitelist successfully",
		Entry: &whitelistv1.IPEntry{
			Ip:          req.Ip,
			Description: req.Description,
			AddedBy:     req.AddedBy,
			AddedAt:     timestamppb.Now(),
			ExpiresAt:   expiresAt,
		},
	}, nil
}

// RemoveFromWhitelist removes an IP/CIDR from the whitelist
func (s *WhitelistServiceServer) RemoveFromWhitelist(ctx context.Context, req *whitelistv1.RemoveFromWhitelistRequest) (*whitelistv1.RemoveFromWhitelistResponse, error) {
	err := s.manager.RemoveFromWhitelist(req.Ip, "grpc-api")
	if err != nil {
		return &whitelistv1.RemoveFromWhitelistResponse{
			Success: false,
			Message: err.Error(),
		}, nil
	}

	return &whitelistv1.RemoveFromWhitelistResponse{
		Success: true,
		Message: "IP removed from whitelist successfully",
	}, nil
}

// GetWhitelist returns all whitelisted IPs
func (s *WhitelistServiceServer) GetWhitelist(ctx context.Context, req *whitelistv1.GetWhitelistRequest) (*whitelistv1.GetWhitelistResponse, error) {
	entries := s.manager.GetWhitelist()

	protoEntries := make([]*whitelistv1.IPEntry, len(entries))
	for i, entry := range entries {
		protoEntries[i] = convertToProtoIPEntry(entry)
	}

	return &whitelistv1.GetWhitelistResponse{
		Entries: protoEntries,
	}, nil
}

// AddToBlacklist adds an IP/CIDR to the blacklist
func (s *WhitelistServiceServer) AddToBlacklist(ctx context.Context, req *whitelistv1.AddToBlacklistRequest) (*whitelistv1.AddToBlacklistResponse, error) {
	err := s.manager.AddToBlacklist(req.Ip, req.Description, req.AddedBy)
	if err != nil {
		return &whitelistv1.AddToBlacklistResponse{
			Success: false,
			Message: err.Error(),
		}, nil
	}

	return &whitelistv1.AddToBlacklistResponse{
		Success: true,
		Message: "IP added to blacklist successfully",
		Entry: &whitelistv1.IPEntry{
			Ip:          req.Ip,
			Description: req.Description,
			AddedBy:     req.AddedBy,
			AddedAt:     timestamppb.Now(),
		},
	}, nil
}

// RemoveFromBlacklist removes an IP/CIDR from the blacklist
func (s *WhitelistServiceServer) RemoveFromBlacklist(ctx context.Context, req *whitelistv1.RemoveFromBlacklistRequest) (*whitelistv1.RemoveFromBlacklistResponse, error) {
	err := s.manager.RemoveFromBlacklist(req.Ip, "grpc-api")
	if err != nil {
		return &whitelistv1.RemoveFromBlacklistResponse{
			Success: false,
			Message: err.Error(),
		}, nil
	}

	return &whitelistv1.RemoveFromBlacklistResponse{
		Success: true,
		Message: "IP removed from blacklist successfully",
	}, nil
}

// GetBlacklist returns all blacklisted IPs
func (s *WhitelistServiceServer) GetBlacklist(ctx context.Context, req *whitelistv1.GetBlacklistRequest) (*whitelistv1.GetBlacklistResponse, error) {
	entries := s.manager.GetBlacklist()

	protoEntries := make([]*whitelistv1.IPEntry, len(entries))
	for i, entry := range entries {
		protoEntries[i] = convertToProtoIPEntry(entry)
	}

	return &whitelistv1.GetBlacklistResponse{
		Entries: protoEntries,
	}, nil
}

// IsAllowed checks if an IP is allowed (not blacklisted and whitelisted)
func (s *WhitelistServiceServer) IsAllowed(ctx context.Context, req *whitelistv1.IsAllowedRequest) (*whitelistv1.IsAllowedResponse, error) {
	allowed := s.manager.IsAllowed(req.Ip)
	reason := ""
	if !allowed {
		reason = "IP not in whitelist or in blacklist"
	}

	return &whitelistv1.IsAllowedResponse{
		Allowed: allowed,
		Reason:  reason,
	}, nil
}

// CleanupExpired removes expired whitelist/blacklist entries
func (s *WhitelistServiceServer) CleanupExpired(ctx context.Context, req *whitelistv1.CleanupExpiredRequest) (*whitelistv1.CleanupExpiredResponse, error) {
	removed := s.manager.CleanupExpired()

	return &whitelistv1.CleanupExpiredResponse{
		WhitelistRemoved: int32(removed),
		BlacklistRemoved: 0, // Current implementation doesn't separate counts
		Message:          fmt.Sprintf("Removed %d expired entries", removed),
	}, nil
}

// Helper functions

func convertToProtoIPEntry(entry whitelist.IPEntry) *whitelistv1.IPEntry {
	protoEntry := &whitelistv1.IPEntry{
		Ip:          entry.IP,
		Description: entry.Description,
		AddedBy:     entry.AddedBy,
		AddedAt:     timestamppb.New(entry.AddedAt),
	}

	if entry.ExpiresAt != nil {
		protoEntry.ExpiresAt = timestamppb.New(*entry.ExpiresAt)
		protoEntry.Expired = entry.ExpiresAt.Before(time.Now())
	}

	return protoEntry
}
