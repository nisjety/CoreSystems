package http

import (
	"net/http"
	"os"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// W3-DEC-KIND (decision D17): POST /api/v1/internal/spaces/space-kind-evidence, registered through the P-UC1 seam. The
// gateway asks for it under the caller's own delegation, immediately before it restores a withheld note. The body names
// only the Space. The organization, the subject and the kind are Control's facts for the caller, resolved here. A Space
// the caller cannot read is refused, and no claim is signed for it.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/space-kind-evidence", s.requireVerifiedSpaceResolver, s.issueSpaceKindEvidence)
	})
}

type spaceKindEvidenceRequest struct {
	SpaceRef string `json:"space_ref"`
}

// spaceKindEvidenceDeniedMessage is the one fixed refusal string the route answers with, for every refusal to read.
const spaceKindEvidenceDeniedMessage = "Space kind evidence is not available for this Space"

// issueSpaceKindEvidence signs the kind of one readable Space for the caller. The readable resolver answers for the
// caller alone, and a Space they cannot read answers 403 with no claim.
func (s *Server) issueSpaceKindEvidence(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request spaceKindEvidenceRequest
	if !bindSpaceDecisionBody(c, &request, "space_ref is required", &request.SpaceRef) {
		return
	}
	membership, err := s.spaceRepo.ResolveReadableUserMembership(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectMembershipOnly, spaceKindEvidenceDeniedMessage,
		http.StatusServiceUnavailable, "Space authority unavailable") {
		return
	}
	evidence, err := spaces.IssueSpaceKindEvidence(membership, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, spaceKindEvidenceDeniedMessage)
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	token, err := spaces.SignSpaceKindEvidence(key, evidence)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"evidence": evidence, "token": token}})
}
