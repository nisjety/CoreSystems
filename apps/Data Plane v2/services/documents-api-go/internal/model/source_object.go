package model

import (
	"encoding/json"
	"time"
)

type SourceObject struct {
	SourceObjectID string          `json:"source_object_id"`
	OrgID          string          `json:"org_id"`
	Connector      string          `json:"connector"`
	Source         string          `json:"source"`
	ExternalID     string          `json:"external_id"`
	SiteID         string          `json:"site_id,omitempty"`
	DriveID        string          `json:"drive_id,omitempty"`
	ItemID         string          `json:"item_id,omitempty"`
	ParentID       string          `json:"parent_id,omitempty"`
	Path           string          `json:"path,omitempty"`
	Name           string          `json:"name"`
	MimeType       string          `json:"mime_type,omitempty"`
	SizeBytes      *int64          `json:"size_bytes,omitempty"`
	ETag           string          `json:"etag,omitempty"`
	CTag           string          `json:"ctag,omitempty"`
	QuickXorHash   string          `json:"quickxor_hash,omitempty"`
	SHA1Hash       string          `json:"sha1_hash,omitempty"`
	ContentHash    string          `json:"content_hash,omitempty"`
	ACLTags        []string        `json:"acl_tags"`
	Metadata       json.RawMessage `json:"metadata"`
	ModifiedAt     *time.Time      `json:"modified_at,omitempty"`
	DiscoveredAt   time.Time       `json:"discovered_at"`
	DeletedAt      *time.Time      `json:"deleted_at,omitempty"`
	UpdatedAt      time.Time       `json:"updated_at"`
}

type UpsertSourceObjectInput struct {
	OrgID        string          `json:"org_id,omitempty"`
	Connector    string          `json:"connector"`
	Source       string          `json:"source"`
	ExternalID   string          `json:"external_id"`
	SiteID       string          `json:"site_id,omitempty"`
	DriveID      string          `json:"drive_id,omitempty"`
	ItemID       string          `json:"item_id,omitempty"`
	ParentID     string          `json:"parent_id,omitempty"`
	Path         string          `json:"path,omitempty"`
	Name         string          `json:"name"`
	MimeType     string          `json:"mime_type,omitempty"`
	SizeBytes    *int64          `json:"size_bytes,omitempty"`
	ETag         string          `json:"etag,omitempty"`
	CTag         string          `json:"ctag,omitempty"`
	QuickXorHash string          `json:"quickxor_hash,omitempty"`
	SHA1Hash     string          `json:"sha1_hash,omitempty"`
	ContentHash  string          `json:"content_hash,omitempty"`
	ACLTags      []string        `json:"acl_tags,omitempty"`
	Metadata     json.RawMessage `json:"metadata,omitempty"`
	ModifiedAt   *time.Time      `json:"modified_at,omitempty"`
}

type DeleteSourceObjectInput struct {
	OrgID          string `json:"org_id,omitempty"`
	SourceObjectID string `json:"source_object_id,omitempty"`
	Connector      string `json:"connector,omitempty"`
	ExternalID     string `json:"external_id,omitempty"`
}

type ListSourceObjectDuplicatesInput struct {
	OrgID        string
	Source       string
	MinCount     int
	MinSizeBytes int64
	MaxGroups    int
}

type SourceObjectDuplicateGroup struct {
	HashKind   string                        `json:"hash_kind"`
	HashValue  string                        `json:"hash_value"`
	Count      int                           `json:"count"`
	TotalBytes int64                         `json:"total_bytes"`
	Members    []SourceObjectDuplicateMember `json:"members"`
}

type SourceObjectDuplicateMember struct {
	SourceObjectID string     `json:"source_object_id"`
	Connector      string     `json:"connector"`
	Source         string     `json:"source"`
	ExternalID     string     `json:"external_id"`
	SiteID         string     `json:"site_id,omitempty"`
	DriveID        string     `json:"drive_id,omitempty"`
	ItemID         string     `json:"item_id,omitempty"`
	Path           string     `json:"path,omitempty"`
	Name           string     `json:"name"`
	MimeType       string     `json:"mime_type,omitempty"`
	SizeBytes      int64      `json:"size_bytes"`
	ModifiedAt     *time.Time `json:"modified_at,omitempty"`
	QuickXorHash   string     `json:"quickxor_hash,omitempty"`
	SHA1Hash       string     `json:"sha1_hash,omitempty"`
	ContentHash    string     `json:"content_hash,omitempty"`
}
