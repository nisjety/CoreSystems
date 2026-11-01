package http

import "github.com/gin-gonic/gin"

// Route registration seam (plan P-UC1). New Control routes live in their own
// routes_<feature>.go and register themselves from an init() through one of
// the two hooks below, so no feature edits server.go or this file.
//
//   - registerSpaceFeatureRoute: routes under /api/v1/internal/spaces. The group
//     already sits behind requireServicePrincipal; each route still names its
//     own scope guard (require*) exactly as the routes below do.
//   - registerFeatureRoute: routes on /api/v1 or /api/v1/internal outside the
//     spaces group.
//
// Every internal route must also have its scope in scopes_spaces.go (spaces)
// or service_auth.go (everything else); an unmapped internal path falls to a
// users:* default scope.
type spaceRouteRegistrar func(s *Server, spaces *gin.RouterGroup)

type featureRouteRegistrar func(s *Server, v1 *gin.RouterGroup, internal *gin.RouterGroup)

var (
	spaceFeatureRouteRegistrars []spaceRouteRegistrar
	featureRouteRegistrars      []featureRouteRegistrar
)

func registerSpaceFeatureRoute(register spaceRouteRegistrar) {
	spaceFeatureRouteRegistrars = append(spaceFeatureRouteRegistrars, register)
}

func registerFeatureRoute(register featureRouteRegistrar) {
	featureRouteRegistrars = append(featureRouteRegistrars, register)
}

func (s *Server) registerFeatureRoutes(v1 *gin.RouterGroup, internal *gin.RouterGroup) {
	for _, register := range featureRouteRegistrars {
		register(s, v1, internal)
	}
}

// registerSpaceRoutes registers the /api/v1/internal/spaces routes (moved
// unchanged from server.go setupRoutes), then every feature registrar.
func (s *Server) registerSpaceRoutes(spacesGroup *gin.RouterGroup) {
	spacesGroup.POST("/register", s.requireSpaceLifecycleRegistrar, s.registerSpace)
	spacesGroup.POST("/deletion-authorizations", s.requireSpaceDeletionAuthorizer, s.authorizeSpaceDeletion)
	spacesGroup.PUT("/deletion-policy", s.requireSpacePolicyWriter, s.upsertSpaceDeletionPolicy)
	spacesGroup.PUT("/:space_ref/legal-hold", s.requireSpacePolicyWriter, s.applySpaceLegalHold)
	spacesGroup.DELETE("/:space_ref/legal-hold", s.requireSpacePolicyWriter, s.releaseSpaceLegalHold)
	spacesGroup.GET("", s.requireVerifiedSpaceResolver, s.listSpacesForSubject)
	spacesGroup.GET("/:space_ref/roster", s.requireVerifiedSpaceResolver, s.spaceRoster)
	spacesGroup.PUT("/:space_ref/memberships", s.requireSpaceMembershipWriter, s.replaceSpaceMemberships)
	spacesGroup.POST("/recipient-audiences", s.requireSpaceAudiencePublisher, s.registerRecipientAudience)
	spacesGroup.PUT("/effect-policy", s.requireSpacePolicyWriter, s.upsertSpaceEffectPolicy)
	spacesGroup.GET("/:space_ref/membership", s.requireVerifiedSpaceResolver, s.resolveCurrentSpaceMembership)
	spacesGroup.POST("/personal-thread-decision", s.requireVerifiedSpaceResolver, s.issuePersonalThreadDecision)
	spacesGroup.POST("/thread-decision", s.requireVerifiedSpaceResolver, s.issueThreadDecision)
	spacesGroup.POST("/thread-append-decision", s.requireVerifiedSpaceResolver, s.issueThreadAppendDecision)
	spacesGroup.POST("/thread-read-decision", s.requireVerifiedSpaceResolver, s.issueThreadReadDecision)
	spacesGroup.POST("/personal-retrieval-decision", s.requireVerifiedSpaceResolver, s.issuePersonalRetrievalDecision)
	spacesGroup.POST("/retrieval-decision", s.requireVerifiedSpaceResolver, s.issueRetrievalDecision)
	spacesGroup.POST("/personal-import-decision", s.requireVerifiedSpaceResolver, s.issuePersonalImportDecision)
	spacesGroup.POST("/schedule-create-decision", s.requireVerifiedSpaceResolver, s.issueScheduleCreateDecision)
	// S4.3 watches. Create is gateway-delegated (a member asks);
	// observe is service-delegated (the sweeper asks, immediately
	// before it records anything a human can read).
	spacesGroup.POST("/watch-create-decision", s.requireVerifiedSpaceResolver, s.issueWatchCreateDecision)
	spacesGroup.POST("/watch-observe-decision", s.requireVerifiedSpaceResolver, s.issueWatchObserveDecision)
	spacesGroup.POST("/import-execution-decision", s.requireSpaceImportReauthorizer, s.issuePersonalImportExecutionDecision)
	spacesGroup.POST("/schedule-fire-decision", s.requireSpaceScheduleFireReauthorizer, s.issueScheduleFireDecision)
	spacesGroup.POST("/sandbox-capability-decision", s.requireSpaceSandboxCapabilityReauthorizer, s.issueSpaceCapabilityDecision)
	spacesGroup.POST("/scheduled-run-decision", s.requireSpaceScheduleFireReauthorizer, s.issueScheduledRunDecision)
	spacesGroup.POST("/scheduled-run-execution-decision", s.requireSpaceScheduledRunExecutor, s.issueScheduledRunExecutionDecision)
	spacesGroup.POST("/scheduled-step-decision", s.requireSpaceScheduledStepExecutor, s.issueScheduledStepDecision)
	spacesGroup.POST("/owner-grant-decision", s.requireVerifiedSpaceResolver, s.issueOwnerGrantDecision)
	spacesGroup.POST("/model-action-view", s.requireSpaceAgentActionViewer, s.issueModelActionView)
	spacesGroup.POST("/run-action-decision", s.requireSpaceAgentActionAuthorizer, s.issueRunActionDecision)
	spacesGroup.POST("/run-action-authority-check", s.requireCurrentRunActionAuthorityChecker, s.checkCurrentRunActionAuthority)
	spacesGroup.POST("/owner-effect-reservations/reserve", s.requireOwnerEffectReservationCoordinator, s.reserveOwnerEffect)
	spacesGroup.POST("/owner-effect-reservations/:reservation_id/commit", s.requireOwnerEffectReservationCoordinator, s.commitOwnerEffectReservation)
	spacesGroup.GET("/owner-effect-reservations/:reservation_id", s.requireOwnerEffectReservationCoordinator, s.getOwnerEffectReservation)
	for _, register := range spaceFeatureRouteRegistrars {
		register(s, spacesGroup)
	}
}
