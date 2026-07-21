package sharepoint

import (
	"context"
	"errors"
	"time"
)

var ErrNotConfigured = errors.New("sharepoint integration not configured")

type Site struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	DisplayName string `json:"display_name,omitempty"`
	WebURL      string `json:"web_url,omitempty"`
	Description string `json:"description,omitempty"`
}

type Item struct {
	Name         string    `json:"name"`
	Path         string    `json:"path"`
	Size         int64     `json:"size"`
	IsFolder     bool      `json:"is_folder"`
	ModifiedTime time.Time `json:"modified_time,omitempty"`
	WebURL       string    `json:"web_url,omitempty"`
}

// Drive is one SharePoint/OneDrive document library on a site — the unit a
// finspo source is registered against. `ID` is the Graph drive id the register
// form needs; `DriveType` is Graph's `driveType` (documentLibrary, business,
// personal, …). Surfaced by ListDrives so the UI can present a pick-a-library
// step instead of asking the user to hand-enter a raw `b!…` drive id.
type Drive struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	DriveType string `json:"drive_type,omitempty"`
	WebURL    string `json:"web_url,omitempty"`
}

type Browser interface {
	ListSites(ctx context.Context, organizationID string) ([]Site, error)
	ListDrives(ctx context.Context, organizationID string, siteID string) ([]Drive, error)
	ListItems(ctx context.Context, organizationID string, siteID string, path string) ([]Item, error)
}

type DisconnectedBrowser struct{}

func (DisconnectedBrowser) ListSites(context.Context, string) ([]Site, error) {
	return nil, ErrNotConfigured
}

func (DisconnectedBrowser) ListDrives(context.Context, string, string) ([]Drive, error) {
	return nil, ErrNotConfigured
}

func (DisconnectedBrowser) ListItems(context.Context, string, string, string) ([]Item, error) {
	return nil, ErrNotConfigured
}

// ---------------------------------------------------------------------------
// Graph DriveItem shapes for the delta sync path.
//
// We unmarshal a subset of the Microsoft Graph DriveItem resource. Any field
// not declared here is preserved in Raw for forensic inspection if needed.
// ---------------------------------------------------------------------------

// Hashes mirrors the subset of file.hashes Microsoft exposes. Graph documents
// quickXorHash + sha1Hash; sha256Hash is explicitly not supported and is
// intentionally omitted here.
type Hashes struct {
	QuickXorHash string `json:"quickXorHash,omitempty"`
	SHA1Hash     string `json:"sha1Hash,omitempty"`
}

type FileFacet struct {
	MimeType string `json:"mimeType,omitempty"`
	Hashes   Hashes `json:"hashes,omitzero"`
}

// FolderFacet is empty in practice; presence alone signals "this is a folder".
type FolderFacet struct {
	ChildCount int64 `json:"childCount,omitempty"`
}

// DeletedFacet — when present, the item has been removed from the drive.
type DeletedFacet struct {
	State string `json:"state,omitempty"`
}

type ParentReference struct {
	DriveID string `json:"driveId,omitempty"`
	ID      string `json:"id,omitempty"`
	Path    string `json:"path,omitempty"`
}

// DriveItem captures the fields finspo persists in the items table. Raw holds
// the unparsed JSON so we can backfill new columns without re-pulling deltas.
type DriveItem struct {
	ID                   string           `json:"id"`
	Name                 string           `json:"name"`
	Size                 int64            `json:"size"`
	WebURL               string           `json:"webUrl,omitempty"`
	ETag                 string           `json:"eTag,omitempty"`
	CTag                 string           `json:"cTag,omitempty"`
	LastModifiedDateTime *time.Time       `json:"lastModifiedDateTime,omitempty"`
	File                 *FileFacet       `json:"file,omitempty"`
	Folder               *FolderFacet     `json:"folder,omitempty"`
	Deleted              *DeletedFacet    `json:"deleted,omitempty"`
	Parent               *ParentReference `json:"parentReference,omitempty"`
}

// IsFolder reports whether this DriveItem represents a folder.
func (d DriveItem) IsFolder() bool { return d.Folder != nil }

// IsDeleted reports whether Graph has marked this item as removed.
func (d DriveItem) IsDeleted() bool { return d.Deleted != nil }

// QuickXorHash returns the quickXorHash if present, otherwise "".
func (d DriveItem) QuickXorHash() string {
	if d.File == nil {
		return ""
	}
	return d.File.Hashes.QuickXorHash
}

// SHA1Hash returns the sha1Hash if present, otherwise "".
func (d DriveItem) SHA1Hash() string {
	if d.File == nil {
		return ""
	}
	return d.File.Hashes.SHA1Hash
}

// MimeType returns the file's MIME type if known.
func (d DriveItem) MimeType() string {
	if d.File == nil {
		return ""
	}
	return d.File.MimeType
}

// ParentItemID returns the parent DriveItem ID, or "" when unknown.
func (d DriveItem) ParentItemID() string {
	if d.Parent == nil {
		return ""
	}
	return d.Parent.ID
}

// FullPath joins the parent path with the item name. Graph returns parent
// paths in the form "/drives/<id>/root:/folder/subfolder" — the segment
// after the ":" is what we want.
func (d DriveItem) FullPath() string {
	if d.Parent == nil || d.Parent.Path == "" {
		return "/" + d.Name
	}
	parent := d.Parent.Path
	if idx := indexOfColon(parent); idx >= 0 {
		parent = parent[idx+1:]
	}
	if parent == "" {
		parent = "/"
	}
	if parent == "/" {
		return "/" + d.Name
	}
	return parent + "/" + d.Name
}

func indexOfColon(s string) int {
	for i := 0; i < len(s); i++ {
		if s[i] == ':' {
			return i
		}
	}
	return -1
}

// DeltaPage is one response page from the /drives/{id}/root/delta endpoint.
type DeltaPage struct {
	Items     []DriveItem
	NextLink  string
	DeltaLink string
}

// ---------------------------------------------------------------------------
// Graph permissions shapes for ACL capture (Phase 3).
//
// We intentionally model the subset finspo persists. Field naming follows the
// Microsoft Graph v1.0 permission resource. Anything unmapped lives in Raw so
// new columns can be added without re-pulling permissions.
// ---------------------------------------------------------------------------

// PermissionIdentity describes one user, group, or device referenced inside a
// grantedToV2 / grantedToIdentitiesV2 entry.
type PermissionIdentity struct {
	ID          string `json:"id,omitempty"`
	DisplayName string `json:"displayName,omitempty"`
}

// PermissionIdentitySet mirrors the Graph IdentitySet shape — at most one of
// the three fields is populated per principal.
type PermissionIdentitySet struct {
	User        *PermissionIdentity `json:"user,omitempty"`
	Group       *PermissionIdentity `json:"group,omitempty"`
	Application *PermissionIdentity `json:"application,omitempty"`
	SiteUser    *PermissionIdentity `json:"siteUser,omitempty"`
	SiteGroup   *PermissionIdentity `json:"siteGroup,omitempty"`
}

func (s PermissionIdentitySet) Principal() (id, kind, name string) {
	switch {
	case s.User != nil:
		return s.User.ID, "user", s.User.DisplayName
	case s.Group != nil:
		return s.Group.ID, "group", s.Group.DisplayName
	case s.Application != nil:
		return s.Application.ID, "application", s.Application.DisplayName
	case s.SiteUser != nil:
		return s.SiteUser.ID, "siteUser", s.SiteUser.DisplayName
	case s.SiteGroup != nil:
		return s.SiteGroup.ID, "siteGroup", s.SiteGroup.DisplayName
	}
	return "", "", ""
}

// PermissionLink describes a shared link grant (scope = anonymous|organization|users).
type PermissionLink struct {
	Scope string `json:"scope,omitempty"`
	Type  string `json:"type,omitempty"`
}

// PermissionEntry is one permission returned by /drives/{id}/items/{id}/permissions.
type PermissionEntry struct {
	ID                    string                 `json:"id,omitempty"`
	Roles                 []string               `json:"roles,omitempty"`
	GrantedToV2           *PermissionIdentitySet `json:"grantedToV2,omitempty"`
	GrantedToIdentitiesV2 []PermissionIdentitySet `json:"grantedToIdentitiesV2,omitempty"`
	Link                  *PermissionLink        `json:"link,omitempty"`
	InheritedFrom         *ParentReference       `json:"inheritedFrom,omitempty"`
}

// IsInherited reports whether this permission was inherited from a parent
// (folder/drive) rather than granted directly on the item.
func (p PermissionEntry) IsInherited() bool { return p.InheritedFrom != nil }

// PermissionsPage is one response page from the permissions endpoint.
type PermissionsPage struct {
	Permissions []PermissionEntry
	NextLink    string
}
