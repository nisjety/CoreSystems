package dataplane

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/triodelab/finspo/internal/store"
)

type SourceObjectClient struct {
	baseURL    string
	apiKey     string
	httpClient *http.Client
}

func NewSourceObjectClient(baseURL, apiKey string) *SourceObjectClient {
	return &SourceObjectClient{
		baseURL: strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		apiKey:  strings.TrimSpace(apiKey),
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

func (c *SourceObjectClient) UpsertSourceObject(ctx context.Context, source store.Source, item store.Item) error {
	return c.UpsertSourceObjectWithPermissions(ctx, source, item, nil)
}

func (c *SourceObjectClient) UpsertSourceObjectWithPermissions(ctx context.Context, source store.Source, item store.Item, permissions []store.Permission) error {
	if c == nil || c.baseURL == "" {
		return nil
	}

	payload := sourceObjectPayload(source, item, permissions)
	return c.post(ctx, source.OrganizationID, "/v1/source-objects/", payload)
}

func (c *SourceObjectClient) DeleteSourceObject(ctx context.Context, source store.Source, item store.Item) error {
	if c == nil || c.baseURL == "" {
		return nil
	}

	payload := map[string]string{
		"connector":   "sharepoint",
		"external_id": externalID(source, item.ItemID),
	}
	return c.post(ctx, source.OrganizationID, "/v1/source-objects/delete", payload)
}

func (c *SourceObjectClient) post(ctx context.Context, orgID, path string, payload any) error {
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal source object payload: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+path, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build data plane request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Org-ID", orgID)
	if c.apiKey != "" {
		req.Header.Set("X-Internal-Api-Key", c.apiKey)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("data plane source object request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("data plane source object request returned %s", resp.Status)
	}
	return nil
}

func sourceObjectPayload(source store.Source, item store.Item, permissions []store.Permission) map[string]any {
	contentHash := ""
	if item.SHA1Hash != "" {
		contentHash = "sha1:" + item.SHA1Hash
	} else if item.QuickXorHash != "" {
		contentHash = "quickxor:" + item.QuickXorHash
	}

	metadata := map[string]any{
		"source_id":    source.ID.String(),
		"tenant_id":    source.TenantID,
		"site_web_url": source.SiteWebURL,
		"drive_name":   source.DriveName,
		"drive_type":   source.DriveType,
		"web_url":      item.WebURL,
		"is_folder":    item.IsFolder,
	}

	return map[string]any{
		"connector":     "sharepoint",
		"source":        "sharepoint",
		"external_id":   externalID(source, item.ItemID),
		"site_id":       source.SiteID,
		"drive_id":      source.DriveID,
		"item_id":       item.ItemID,
		"parent_id":     item.ParentItemID,
		"path":          item.Path,
		"name":          item.Name,
		"mime_type":     item.MimeType,
		"size_bytes":    item.SizeBytes,
		"etag":          item.ETag,
		"ctag":          item.CTag,
		"quickxor_hash": item.QuickXorHash,
		"sha1_hash":     item.SHA1Hash,
		"content_hash":  contentHash,
		"acl_tags":      aclTags(source, permissions),
		"metadata":      metadata,
		"modified_at":   item.ModifiedAt,
	}
}

func externalID(source store.Source, itemID string) string {
	return source.DriveID + ":" + itemID
}

func aclTags(source store.Source, permissions []store.Permission) []string {
	tags := []string{
		"connector:sharepoint",
		"org:" + source.OrganizationID,
	}
	if source.SiteID != "" {
		tags = append(tags, "site:"+source.SiteID)
	}
	if source.DriveID != "" {
		tags = append(tags, "drive:"+source.DriveID)
	}
	for _, permission := range permissions {
		principalType := normalizeTagValue(permission.PrincipalType)
		principalID := normalizeTagValue(permission.PrincipalID)
		if principalID != "" {
			if principalType == "" {
				principalType = "unknown"
			}
			tags = append(tags, "sp:principal:"+principalType+":"+principalID)
		}
		for _, role := range permission.Roles {
			if value := normalizeTagValue(role); value != "" {
				tags = append(tags, "sp:role:"+value)
			}
		}
		if value := normalizeTagValue(permission.LinkScope); value != "" {
			tags = append(tags, "sp:link_scope:"+value)
		}
		if value := normalizeTagValue(permission.LinkType); value != "" {
			tags = append(tags, "sp:link_type:"+value)
		}
		if value := normalizeTagValue(permission.InheritedFrom); value != "" {
			tags = append(tags, "sp:inherited_from:"+value)
		}
	}
	tags = dedupeSorted(tags)
	return tags
}

func normalizeTagValue(value string) string {
	value = strings.TrimSpace(strings.ToLower(value))
	if value == "" {
		return ""
	}
	return strings.Map(func(r rune) rune {
		switch {
		case r >= 'a' && r <= 'z':
			return r
		case r >= '0' && r <= '9':
			return r
		case r == '-' || r == '_' || r == '.' || r == ':' || r == '@':
			return r
		default:
			return '_'
		}
	}, value)
}

func dedupeSorted(tags []string) []string {
	if len(tags) == 0 {
		return nil
	}
	out := make([]string, 0, len(tags))
	seen := make(map[string]struct{}, len(tags))
	for _, tag := range tags {
		tag = strings.TrimSpace(tag)
		if tag == "" {
			continue
		}
		if _, ok := seen[tag]; ok {
			continue
		}
		seen[tag] = struct{}{}
		out = append(out, tag)
	}
	sort.Strings(out)
	return out
}
