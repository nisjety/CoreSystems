package http

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"
	"unicode/utf8"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/directory"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// A-REC-03 (F-t1-049): the run-bound people read behind the agent directory
// resolver. POST /api/v1/internal/spaces/run-directory/people is the Control
// half of the space.directory.resolve tool that execution-core runs. The caller
// is the execution-core workload with scope spaces:directory:run-read; it names
// only the run (and an opaque org selector) and the text to look up. Control
// resolves the run through Session Core, so the organization, the subject the
// search runs as and the room come from the run, never from the request. The
// subject must still be a current member of the run's room, and the golden row
// plus the operator allowlist (CONTROL_SPACE_SELF_API_ACTIONS) must admit the
// action at the member's role.
//
// The answer is presence, never authority: ids, names and emails of
// organization members, each with in_room (whether the person is in the run's
// own room). It never names another room. Nothing is persisted, the response is
// no-store, and zero_data_retention tells execution-core to hand the model ids
// only. A Control or Session Core outage is a typed 503, never an empty list.
const (
	runDirectoryReadScope   = "spaces:directory:run-read"
	runDirectoryActionID    = "space.directory.resolve"
	runDirectoryPeopleLimit = 50
	runDirectoryMaxQuery    = 100
	runDirectoryMaxID       = 200
)

// runDirectoryMembership is the run subject's current place in its room.
type runDirectoryMembership struct {
	Role              string
	ZeroDataRetention bool
}

// runDirectoryMembershipFunc reads the current membership of subjectID in
// spaceRef. spaces.ErrNoCurrentMembership means the subject is not a member.
type runDirectoryMembershipFunc func(ctx context.Context, spaceRef, orgID, subjectID string) (runDirectoryMembership, error)

// runDirectoryAdmitted is the registry and allowlist check; tests replace it
// while the golden row is not vendored yet.
var runDirectoryAdmitted = spaces.RunActionReadAdmitted

type runDirectoryRequest struct {
	RunID string `json:"run_id"`
	OrgID string `json:"org_id"`
	Query string `json:"query"`
}

type runDirectoryPerson struct {
	SubjectID   string `json:"subject_id"`
	DisplayName string `json:"display_name"`
	Email       string `json:"email"`
	InRoom      bool   `json:"in_room"`
}

func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/run-directory/people", s.requireRunDirectoryReader, s.runDirectoryPeople)
	})
}

// requireRunDirectoryReader admits only execution-core with the run-directory scope.
func (s *Server) requireRunDirectoryReader(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != executionCorePrincipal ||
		!hasServiceScope(c, runDirectoryReadScope) {
		abortTypedError(c, http.StatusForbidden, "run_directory_reader_required",
			"the Execution Core run directory reader is required", nil)
		return
	}
	c.Next()
}

func (s *Server) runDirectoryMembershipOf(ctx context.Context, spaceRef, orgID, subjectID string) (runDirectoryMembership, error) {
	if s.runDirectoryMembership != nil {
		return s.runDirectoryMembership(ctx, spaceRef, orgID, subjectID)
	}
	if s.spaceRepo == nil {
		return runDirectoryMembership{}, errors.New("space repository is unavailable")
	}
	evidence, err := s.spaceRepo.ResolveAgentActionDecisionEvidence(ctx, spaceRef, orgID, subjectID)
	if err != nil {
		return runDirectoryMembership{}, err
	}
	return runDirectoryMembership{
		Role:              evidence.Membership.Role,
		ZeroDataRetention: evidence.Privacy.ZeroDataRetention,
	}, nil
}

func runDirectoryInvalid(c *gin.Context, field, reason string) {
	abortTypedError(c, http.StatusBadRequest, "invalid_request", "the run directory request is invalid",
		map[string]any{"field": field, "reason": reason})
}

func runDirectoryUnavailable(c *gin.Context) {
	abortTypedError(c, http.StatusServiceUnavailable, "directory_unavailable", "the directory is unavailable", nil)
}

func (s *Server) runDirectoryPeople(c *gin.Context) {
	var request runDirectoryRequest
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, 4*1024))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		runDirectoryInvalid(c, "body", "malformed")
		return
	}
	runID, orgID, query := strings.TrimSpace(request.RunID), strings.TrimSpace(request.OrgID), strings.TrimSpace(request.Query)
	if runID == "" || len(runID) > runDirectoryMaxID {
		runDirectoryInvalid(c, "run_id", "required")
		return
	}
	if orgID == "" || len(orgID) > runDirectoryMaxID {
		runDirectoryInvalid(c, "org_id", "required")
		return
	}
	if query == "" {
		runDirectoryInvalid(c, "query", "required")
		return
	}
	if utf8.RuneCountInString(query) > runDirectoryMaxQuery || strings.ContainsRune(query, 0) {
		runDirectoryInvalid(c, "query", "too_long")
		return
	}
	if s.runActionAuthority == nil || s.peopleDirectory == nil {
		runDirectoryUnavailable(c)
		return
	}
	// org_id is a lookup selector only: Session Core returns the run's own bound
	// organization, subject and room.
	authority, err := s.runActionAuthority.ResolveRunActionAuthority(c.Request.Context(), runID, orgID)
	if err != nil {
		runDirectoryUnavailable(c)
		return
	}
	if authority.Validate() != nil {
		abortTypedError(c, http.StatusForbidden, "not_authorized", "the run may not read the directory", nil)
		return
	}
	membership, err := s.runDirectoryMembershipOf(c.Request.Context(), authority.SpaceRef, authority.OrgID, authority.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		abortTypedError(c, http.StatusForbidden, "not_authorized", "the run may not read the directory", nil)
		return
	}
	if err != nil {
		runDirectoryUnavailable(c)
		return
	}
	if !runDirectoryAdmitted(runDirectoryActionID, membership.Role) {
		abortTypedError(c, http.StatusForbidden, "not_offered", "the directory resolver is not offered to this run", nil)
		return
	}
	base := directory.PeopleQuery{
		OrgID:    authority.OrgID,
		CallerID: authority.SubjectID,
		Q:        query,
		Limit:    runDirectoryPeopleLimit,
	}
	everyone, err := s.peopleDirectory.SearchPeople(c.Request.Context(), base)
	if !s.runDirectoryReadOK(c, err) {
		return
	}
	// The same query with the run's room removed: whoever is missing from it is in the room.
	scoped := base
	scoped.ExcludeMemberOf = authority.SpaceRef
	outside, err := s.peopleDirectory.SearchPeople(c.Request.Context(), scoped)
	if !s.runDirectoryReadOK(c, err) {
		return
	}
	notInRoom := make(map[string]bool, len(outside.People))
	for _, person := range outside.People {
		notInRoom[person.SubjectID] = true
	}
	people := make([]runDirectoryPerson, 0, len(everyone.People))
	for _, person := range everyone.People {
		if strings.TrimSpace(person.SubjectID) == "" {
			continue
		}
		people = append(people, runDirectoryPerson{
			SubjectID:   person.SubjectID,
			DisplayName: person.DisplayName,
			Email:       person.Email,
			InRoom:      !notInRoom[person.SubjectID],
		})
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"people":              people,
		"zero_data_retention": membership.ZeroDataRetention,
	}})
}

// runDirectoryReadOK writes the typed failure of a directory read and reports whether it succeeded.
func (s *Server) runDirectoryReadOK(c *gin.Context, err error) bool {
	switch {
	case err == nil:
		return true
	case errors.Is(err, directory.ErrInvalidRequest):
		runDirectoryInvalid(c, "query", "invalid")
	case errors.Is(err, directory.ErrCallerNotInOrg):
		abortTypedError(c, http.StatusForbidden, "not_authorized", "the run may not read the directory", nil)
	default:
		log.Error().Err(err).Msg("run directory people read failed")
		runDirectoryUnavailable(c)
	}
	return false
}
