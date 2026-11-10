package http

import (
	"context"
	"log"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/brreg"
	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/orgkeys"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/rbac"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/gin-gonic/gin"
)

type Server struct {
	router           *gin.Engine
	httpServer       *http.Server
	orgService       *orgcore.Service
	rbacRepo         *rbac.Repository // U6-3 (ui-ux-verevon-gap.md §10)
	retentionPosture retentionPostureReader
	agentPolicy      agentPolicyReader
	authService      string
	userService      string
	userServiceToken string
	httpClient       *http.Client
	brregClient      *brreg.Client

	// modelEntitlements is set by SetModelEntitlements (WS10-S2); callerRoles is
	// how its admin routes learn the caller's role from the membership table.
	modelEntitlements modelEntitlementService
	callerRoles       callerRoleReader

	// browserSites is set by SetBrowserSites (WS21-S3): the agent-browser site
	// policy of record, read by the broker and administered through these routes.
	browserSites browserSiteService

	// siemEnabled mounts the SIEM destination routes (WS06-S7,
	// ORG_SIEM_DESTINATIONS_ENABLED); siem is the service behind them.
	siemEnabled bool
	siem        siemDestinationService

	// webPolicy is the org web policy of record (WS7.H): administered through
	// /orgs/:id/web-policy and read by Auth Core through
	// /internal/orgs/:orgId/web-policy. Additive and unread until WS7.I.
	webPolicy webPolicyService

	// identityEnabled mounts the domain verification routes (WS05-S6,
	// ORG_DOMAIN_VERIFICATION_ENABLED); identity is the service behind them.
	identityEnabled bool
	identity        identityDomainService
	sso             identitySSOService // SSO policy and providers (WS05-S7), same gate

	// scimOn mounts the SCIM routes (WS05-S8, ORG_SCIM_ENABLED); scim is the
	// service behind them and scimLimiter the per-token rate limit.
	scimOn      bool
	scim        scimService
	scimLimiter *scimRateLimiter

	// keysOn mounts the key custody routes (WS17.E2, ORG_CORE_KEYS_ENABLED,
	// default off); orgKeys is set by SetOrgKeys. With the routes on and no
	// service they answer 503 keys_unavailable.
	keysOn  bool
	orgKeys orgKeyService

	// retentionOn mounts the retention policy and legal hold routes (EP-9 C1,
	// ORG_RETENTION_POLICY_ENABLED, only "true" turns it on); retention is set by
	// SetRetention. Off, none of the routes is mounted.
	retentionOn bool
	retention   retentionService
}

func NewServer(port int, orgService *orgcore.Service, rbacRepo *rbac.Repository, authServiceURL, userServiceURL string) *Server {
	router := gin.New()
	router.Use(gin.Recovery())
	router.Use(correlationMiddleware())
	serviceCredentials, credentialErr := parseServiceCredentials(os.Getenv(serviceCredentialEnv))
	if credentialErr != nil {
		log.Printf("org-core service authentication unavailable: %v", credentialErr)
		serviceCredentials = nil
	}
	if scimEnabled(os.Getenv) && len(serviceCredentials) > 0 {
		serviceCredentials = withSCIMEdgeCredential(serviceCredentials, os.Getenv(scimEdgeTokenEnv))
	}
	router.Use(serviceAuthMiddleware(serviceCredentials, time.Now))

	s := &Server{
		router:           router,
		orgService:       orgService,
		rbacRepo:         rbacRepo,
		authService:      authServiceURL,
		userService:      userServiceURL,
		userServiceToken: strings.TrimSpace(os.Getenv("USER_CORE_SERVICE_TOKEN")),
		httpClient:       &http.Client{Timeout: 10 * time.Second},
		brregClient:      brreg.NewClient(),
	}
	if orgService != nil {
		// Assigned only when present: a typed-nil *Service in the interface
		// would defeat the handler's nil check.
		s.retentionPosture = orgService
		s.agentPolicy = orgService
		s.callerRoles = orgService
		s.webPolicy = orgService
	}
	if siemDestinationsEnabled(os.Getenv) {
		s.siemEnabled = true
		if orgService != nil {
			// Without a usable seal key the routes stay mounted and answer 503: a
			// destination secret is never stored unsealed.
			if sealer, err := orgcore.NewSIEMSealerFromEnv(os.Getenv); err != nil {
				log.Printf("org-core SIEM destinations unavailable: %v", err)
			} else {
				orgService.EnableSIEMDestinations(sealer)
				s.siem = orgService
			}
		}
	}
	if domainVerificationEnabled(os.Getenv) {
		s.identityEnabled = true
		if orgService != nil {
			orgService.EnableDomainVerification(orgcore.NewDomainTXTResolver(domainDNSResolvers(os.Getenv)))
			s.identity = orgService
			s.sso = orgService
			// auth-core owns providers, sign-in evidence and the two-factor flag.
			// Without ORG_CORE_IDENTITY_SERVICE_TOKEN the SSO administration
			// answers 503 and enforce cannot be set.
			if client := orgcore.NewAuthCoreIdentityClient(authCoreIdentityURL(os.Getenv), os.Getenv(authCoreIdentityTokenEnv)); client != nil {
				orgService.SetIdentityProviderAdmin(client)
			} else {
				log.Printf("org-core sso administration: %s is not set; provider administration and enforce are unavailable", authCoreIdentityTokenEnv)
			}
		}
	}
	if scimEnabled(os.Getenv) {
		s.scimOn = true
		s.scimLimiter = newSCIMRateLimiter(scimRatePerMinute, time.Now)
		if orgService != nil {
			// SCIM needs auth-core for every membership change. Without the
			// credential the routes stay mounted and answer 503: a SCIM request
			// never half-applies.
			if client := orgcore.NewAuthCoreIdentityClient(authCoreIdentityURL(os.Getenv), os.Getenv(authCoreIdentityTokenEnv)); client != nil {
				orgService.SetScimProvisioner(client)
				s.scim = orgService
			} else {
				log.Printf("org-core scim: %s is not set; SCIM is unavailable", authCoreIdentityTokenEnv)
			}
		}
	}
	// The key routes exist only when ORG_CORE_KEYS_ENABLED=1; cmd/server wires
	// the service (SetOrgKeys) and refuses to boot without usable master keys
	// (ValidateOrgKeysConfig).
	s.keysOn = orgkeys.Enabled(os.Getenv)
	s.retentionOn = retention.Enabled(os.Getenv)
	s.setupRoutes()
	s.httpServer = &http.Server{
		Addr:              ":" + strconv.Itoa(port),
		Handler:           s.router,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      15 * time.Second,
		IdleTimeout:       30 * time.Second,
	}
	return s
}

func (s *Server) Start() error {
	log.Printf("org-core HTTP listening on %s", s.httpServer.Addr)
	return s.httpServer.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

func (s *Server) setupRoutes() {
	s.router.GET("/health", s.health)

	// guard is the tenant-isolation SECOND LAYER (membership_guard.go): before
	// any org-scoped MUTATION runs, re-verify against org-core's own membership
	// table that the request's x-user-id is an active member of :id. This makes
	// org-core stop blindly trusting the path even behind the scoped service-principal
	// gate. Reads stay gateway-trusted (not wrapped) to avoid a membership
	// round-trip on every GET.
	guard := requireActiveMembership(s.orgService)

	// V1 API routes
	v1 := s.router.Group("/api/v1")
	v1.POST("/auth/login", s.login)
	v1.GET("/users/me", s.getCurrentUser)
	v1.GET("/organizations", s.getUserOrganizations)
	v1.GET("/organizations/:id", s.getOrganization)
	v1.GET("/organizations/:id/entitlements", s.getEntitlements)
	v1.GET("/organizations/:id/quotas", s.getQuotas)
	v1.PUT("/organizations/:id/quotas/:key", guard, s.putQuota)
	v1.GET("/organizations/:id/members/search", s.searchMembers)
	// Chat model policy (WS10-S2, ADR-0005). The handlers check the caller's
	// role against the membership table themselves; the guard is the same
	// second layer the other mutations use.
	v1.GET("/organizations/:id/model-entitlement", s.getModelEntitlement)
	v1.PUT("/organizations/:id/model-entitlement", guard, s.putModelEntitlement)
	v1.GET("/organizations/:id/model-entitlement/explain", s.explainModelEntitlement)
	// Agent-browser site policy (WS21-S3). An admin reads and writes the policy
	// (owner or admin only); a member reads and writes only their own choices. The
	// handlers check the role against the membership table; the guard is the same
	// second layer the other mutations use.
	v1.GET("/organizations/:id/browser-sites", s.getBrowserSites)
	v1.PUT("/organizations/:id/browser-sites", guard, s.putBrowserSites)
	v1.GET("/organizations/:id/browser-sites/explain", s.explainBrowserSite)
	v1.GET("/organizations/:id/browser-sites/me", s.listBrowserSitePrefs)
	// Org web policy (WS7.H). An owner or admin reads and writes the stored
	// document and its history; any other active member reads only the effective
	// document and the entitlements. The handlers check the role against the
	// membership table; the guard is the same second layer the other mutations use.
	v1.GET("/organizations/:id/web-policy", s.getWebPolicy)
	v1.PUT("/organizations/:id/web-policy", guard, s.putWebPolicy)
	v1.GET("/organizations/:id/web-policy/history", s.listWebPolicyHistory)
	v1.PUT("/organizations/:id/browser-sites/me/:host", guard, s.putBrowserSitePref)
	v1.DELETE("/organizations/:id/browser-sites/me/:host", guard, s.deleteBrowserSitePref)
	// createOrganization is intentionally NOT guarded: there is no :id to check
	// membership against yet (the org is being created and the caller becomes
	// its first owner). It still requires x-user-id in the handler.
	v1.POST("/organizations", s.createOrganization)
	v1.POST("/organizations/:id/plan", guard, s.updatePlan)
	v1.PATCH("/organizations/:id/settings", guard, s.updateOrgSettings)
	v1.PATCH("/organizations/:id/brreg", guard, s.verifyOrgBrreg)
	v1.PATCH("/organizations/:id/website", guard, s.updateOrgWebsite)
	v1.GET("/organizations/:id/website", s.getOrgWebsite)
	v1.GET("/brreg/search", s.searchBrreg)
	v1.GET("/brreg/:orgnr", s.lookupBrreg)

	// Frontend proxy routes (without /api/v1 prefix)
	s.router.GET("/orgs/me", s.getUserOrganizations)
	s.router.GET("/orgs", s.getUserOrganizations)
	s.router.GET("/orgs/:id", s.getOrganization)
	s.router.GET("/orgs/:id/entitlements", s.getEntitlements)
	s.router.GET("/orgs/:id/quotas", s.getQuotas)
	s.router.PUT("/orgs/:id/quotas/:key", guard, s.putQuota)
	s.router.POST("/orgs", s.createOrganization)
	s.router.POST("/orgs/:id/plan", guard, s.updatePlan)
	s.router.PATCH("/orgs/:id/capabilities", guard, s.updateCapabilities)
	s.router.GET("/orgs/:id/members", s.listMembers)
	s.router.GET("/orgs/:id/members/search", s.searchMembers)
	s.router.GET("/orgs/:id/model-entitlement", s.getModelEntitlement)
	s.router.PUT("/orgs/:id/model-entitlement", guard, s.putModelEntitlement)
	s.router.GET("/orgs/:id/model-entitlement/explain", s.explainModelEntitlement)
	s.router.GET("/orgs/:id/web-policy", s.getWebPolicy)
	s.router.PUT("/orgs/:id/web-policy", guard, s.putWebPolicy)
	s.router.GET("/orgs/:id/web-policy/history", s.listWebPolicyHistory)

	// SIEM destination config (WS06-S7): owner/admin CRUD. Every path answers 404
	// until ORG_SIEM_DESTINATIONS_ENABLED is on. The handlers check the caller's
	// role against the membership table; the guard is the second layer the other
	// mutations use.
	siemGate := s.requireSIEMEnabled()
	s.router.GET("/orgs/:id/siem-destinations", siemGate, s.listSIEMDestinations)
	s.router.POST("/orgs/:id/siem-destinations", siemGate, guard, s.createSIEMDestination)
	s.router.PATCH("/orgs/:id/siem-destinations/:destinationId", siemGate, guard, s.updateSIEMDestination)
	s.router.DELETE("/orgs/:id/siem-destinations/:destinationId", siemGate, guard, s.deleteSIEMDestination)

	// Domain verification (WS05-S6): owner/admin DNS proof of a company domain. Every
	// path answers 404 until ORG_DOMAIN_VERIFICATION_ENABLED is on. The handlers check
	// the caller's role against the membership table (a platform role is not enough);
	// the guard is the second layer the other mutations use.
	identityGate := s.requireIdentityEnabled()
	s.router.GET("/orgs/:id/domains", identityGate, s.listIdentityDomains)
	s.router.POST("/orgs/:id/domains", identityGate, guard, s.addIdentityDomain)
	s.router.POST("/orgs/:id/domains/:domain/verify", identityGate, guard, s.verifyIdentityDomain)
	s.router.DELETE("/orgs/:id/domains/:domain", identityGate, guard, s.revokeIdentityDomain)

	// SSO policy and provider administration (WS05-S7), same gate and authority.
	s.router.GET("/orgs/:id/sso/policy", identityGate, s.getSSOPolicy)
	s.router.PUT("/orgs/:id/sso/policy", identityGate, guard, s.putSSOPolicy)
	s.router.POST("/orgs/:id/sso/providers", identityGate, guard, s.addSSOProvider)
	s.router.DELETE("/orgs/:id/sso/providers/:providerId", identityGate, guard, s.removeSSOProvider)

	// SCIM token management (WS05-S8): owner/admin, same authority as the domain
	// routes. Every path answers 404 until ORG_SCIM_ENABLED is on.
	scimGate := s.requireSCIMEnabled()
	s.router.GET("/orgs/:id/scim/tokens", scimGate, s.listSCIMTokens)
	s.router.POST("/orgs/:id/scim/tokens", scimGate, guard, s.createSCIMToken)
	s.router.DELETE("/orgs/:id/scim/tokens/:tokenId", scimGate, guard, s.revokeSCIMToken)
	s.router.GET("/orgs/:id/scim/status", scimGate, s.getSCIMStatus)

	// Read-only projection/RBAC catalog. Membership, role assignment, and custom
	// policy mutations are intentionally not mounted for the secure MVP: Auth
	// Core is the sole membership authority, while fine-grained policy editing is
	// deferred to the separately reviewed enterprise phase.
	s.router.GET("/orgs/:id/roles/catalog", s.listCapabilityCatalog)
	s.router.GET("/orgs/:id/roles", s.listRoles)

	// GDPR erasure (owner/admin-gated; calls the gdpr_hard_delete_organization
	// / soft_delete_organization stored procedures). Hard erasure is
	// irreversible and requires { "confirm": true } in the request body.
	// Soft-delete additionally requires the exact org_name as a "type the
	// name to confirm" step. The membership guard runs first (must be a
	// member at all) before the handler's own owner/admin role check.
	s.router.DELETE("/orgs/:id/gdpr/erase", guard, s.hardDeleteOrganization)
	s.router.DELETE("/orgs/:id/gdpr/soft-delete", guard, s.softDeleteOrganization)
	// restore reverses a pending soft-delete; same owner/admin gate as the
	// two erasure routes above (authorizeOrgErasure), so it is wired with the
	// same membership-guard-then-handler-check pattern.
	s.router.POST("/orgs/:id/gdpr/restore", guard, s.restoreOrganization)

	// Flow C self-service surface: any active member may act on their own
	// deletion-ledger row, and a platform admin may act regardless of
	// membership (authorizeDeletionSelfService enforces this itself), so
	// these are deliberately NOT wired behind the membership-only `guard` —
	// that would incorrectly reject a platform admin who isn't a member of
	// this particular org.
	s.router.POST("/orgs/:id/gdpr/deletion/mark-exported", s.markExported)
	s.router.POST("/orgs/:id/gdpr/deletion/acknowledge", s.acknowledgeDeletion)
	s.router.GET("/orgs/:id/gdpr/deletion/status", s.getDeletionStatus)

	// Internal orchestration routes for zero-input enterprise onboarding.
	// These are machine-to-machine (provisioning / onboarding orchestration)
	// and have NO acting end-user (no x-user-id), so the membership guard does
	// not apply — there is no user whose membership could be checked. They stay
	// protected by route-scoped machine principals; access is restricted to
	// explicitly registered control-plane callers and must not be exposed to user traffic.
	internal := s.router.Group("/internal")
	// Enumerates every organization — the auto-discovery source for
	// background services that must service every tenant (no per-request
	// acting user to scope a normal /orgs read to). See listOrganizationsInternal.
	internal.GET("/orgs", s.listOrganizationsInternal)
	internal.GET("/orgs/:orgId", s.getOrganizationInternal)
	// Minimal interactive-retention posture ({zdrIntent, entitled}) for Auth
	// Core's token issuer, behind its own narrow scope org:retention:read:any
	// (not org:read:any, which returns the whole organization record). Computed
	// from the database on every call; see getRetentionPostureInternal.
	internal.GET("/orgs/:orgId/retention-posture", s.getRetentionPostureInternal)
	// The org's memory and learning-review policy for session-core's mirror
	// (WS07-S4), behind its own scope org:agent-policy:read:any; no-store.
	internal.GET("/orgs/:orgId/agent-policy", s.getAgentPolicyInternal)
	// The resolved chat model policy for one org and user, for inference-core's
	// pull (WS10-S2). Own scope org:model-entitlement:read:any; no-store.
	internal.GET("/orgs/:orgId/model-entitlement", s.getModelEntitlementInternal)
	// The resolved agent-browser site decision for the broker's grant mint
	// (WS21-S3) and for the Model Plane's web-domain check (EP-9 W3), which
	// model-gateway and execution-core read. Own scope org:browser-site:read:any,
	// held by browser-broker, model-gateway and execution-core alone; no-store.
	internal.GET("/orgs/:orgId/browser-sites/resolve", s.getBrowserSitesInternal)
	// The org's effective web policy for Auth Core's web_policy token claim
	// (WS7.H, read by WS7.I). Own scope org:web-policy:read:any, held by auth-core
	// alone; no-store.
	internal.GET("/orgs/:orgId/web-policy", s.getWebPolicyInternal)
	// The exporter's read of every enabled SIEM destination, secret unsealed
	// (WS06-S7). Own scope org:siem:read:any, held by audit-core only; no-store.
	// Registered even while the feature is off so the path cannot fall through to
	// /orgs/:orgId and be read as an organisation id.
	internal.GET("/orgs/siem-destinations", siemGate, s.listSIEMDestinationsInternal)
	// Which organisation holds a verified domain (WS05-S6). Own scope
	// org:domain:read:any, held by auth-core alone; no-store.
	internal.GET("/orgs/by-domain/:domain", identityGate, s.getVerifiedDomainOwnerInternal)
	// The SSO policy covering an email domain, for auth-core's enforcement hook
	// (WS05-S7). Same scope; no-store.
	internal.GET("/orgs/by-domain/:domain/sso-policy", identityGate, s.getSSOPolicyByDomainInternal)
	internal.GET("/orgs/:orgId/roles/:roleName/capabilities", s.getEffectiveCapabilities)

	// The SCIM surface (WS05-S8), reached only by the gateway edge under the
	// verevon-scim-edge principal (scope org:scim:any). The IdP bearer token in
	// Authorization names the organisation; Groups are not supported.
	scim := internal.Group("/scim/v2", scimGate, s.scimAuthenticate())
	scim.GET("/ServiceProviderConfig", scimServiceProviderConfig)
	scim.GET("/ResourceTypes", scimResourceTypes)
	scim.GET("/Schemas", scimSchemas)
	scim.GET("/Users", s.scimListUsers)
	scim.POST("/Users", s.scimCreateUser)
	scim.GET("/Users/:id", s.scimGetUser)
	scim.PUT("/Users/:id", s.scimReplaceUser)
	scim.PATCH("/Users/:id", s.scimPatchUser)
	scim.DELETE("/Users/:id", s.scimDeleteUser)
	scim.Any("/Groups", scimNotImplemented)
	scim.Any("/Groups/:id", scimNotImplemented)
	internal.GET("/orgs/by-tenant", s.getOrganizationByTenant)
	internal.POST("/orgs/ensure-from-tenant", s.ensureOrganizationFromTenant)
	internal.POST("/orgs/:orgId/onboarding/state", s.updateOnboardingState)
	internal.POST("/orgs/:orgId/reconcile", s.reconcileOrganizationProjection)
	internal.POST("/orgs/:orgId/members/reconcile", s.reconcileOrganizationMember)
	internal.POST("/orgs/:orgId/reconcile-delete", s.reconcileOrganizationDeletion)
	// Admin-succession handoff: called by user-core's self-erasure pre-flight
	// (EnsureSuccession) before a sole owner/admin's erasure saga starts. See
	// succession_handlers.go.
	internal.POST("/orgs/:orgId/members/:userId/succession", s.promoteMemberSuccession)
	// Key custody (WS17.E2): DEK wrap, unwrap, rewrap, KEK rotation, metadata and
	// crypto-shred. Registered only when ORG_CORE_KEYS_ENABLED=1; see
	// keys_handlers.go for the scope of each route.
	s.registerOrgKeyRoutes(internal)
	// Retention policy and legal holds (EP-9 C1). Mounted only when
	// ORG_RETENTION_POLICY_ENABLED is "true"; see retention_handlers.go.
	s.registerRetentionRoutes(v1, internal, guard)
}
