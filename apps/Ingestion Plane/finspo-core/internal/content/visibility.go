package content

import (
	"strings"

	"github.com/triodelab/finspo/internal/store"
)

// Data Plane v2 document visibility values. `shared` also exists there (grant-
// backed), but a connector cannot express per-principal grants through
// POST /v1/documents, so this classifier only ever chooses between the two it
// can honestly represent.
const (
	VisibilityOrg     = "org"
	VisibilityPrivate = "private"
)

// ClassifyVisibility maps a SharePoint item's captured ACL onto the Data Plane
// visibility the forwarded document should carry.
//
// The rule is a DOWNGRADE rule, not an upgrade rule. Content pulled by an org
// connector out of a shared document library is org knowledge by default —
// that is the whole reason the connector exists, and documents-api's own policy
// says as much ("a system/ingest create … defaults to ORG so shared knowledge
// stays org-visible and doesn't silently vanish"). What this function looks for
// is evidence that a specific item was deliberately locked down, and demotes
// only those to `private`.
//
// An item is treated as org-visible when its ACL shows access reaching beyond
// named individuals:
//
//   - a sharing link scoped to the whole tenant (`organization`) or the public
//     (`anonymous`), or
//   - a grant to a group / SharePoint site group — membership-based access,
//     i.e. "the team" or "everyone with site access".
//
// Everything else is `private`: an item whose only grants are to specific
// people is exactly the case this fix exists to protect.
//
// It FAILS CLOSED. An empty ACL means capture was skipped, failed, or returned
// nothing (permission capture is best-effort in the sync engine and logs
// instead of aborting), and a missing answer must never be read as permission
// to widen access — so it yields `private`.
func ClassifyVisibility(permissions []store.Permission) string {
	if len(permissions) == 0 {
		return VisibilityPrivate
	}
	for _, permission := range permissions {
		switch strings.ToLower(strings.TrimSpace(permission.LinkScope)) {
		case "organization", "anonymous":
			return VisibilityOrg
		}
		// `application` is deliberately excluded: the connector's own service
		// principal appears in nearly every ACL it can read, so counting it
		// would classify every single item as org-visible and make the check
		// meaningless. `user`/`siteUser` are individuals, which is the
		// restricted case.
		switch strings.ToLower(strings.TrimSpace(permission.PrincipalType)) {
		case "group", "sitegroup":
			return VisibilityOrg
		}
	}
	return VisibilityPrivate
}
