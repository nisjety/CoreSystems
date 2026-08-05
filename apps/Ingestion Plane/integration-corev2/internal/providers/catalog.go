package providers

import (
	"sort"
	"strings"
)

type Capability struct {
	Key         string   `json:"key"`
	Label       string   `json:"label"`
	Description string   `json:"description"`
	Scopes      []string `json:"scopes"`
	Sensitive   bool     `json:"sensitive"`
	// Direction is the data-flow direction relative to Verevon: "read" pulls
	// data INTO Verevon (grounding/signals), "write" pushes actions OUT to the
	// provider (replies, publishing, provisioning). Derived from the capability
	// key by capabilityDirection so every provider is modelled consistently and
	// the UI can show read/write both-ways info uniformly.
	Direction string `json:"direction"`
}

// capabilityDirection infers read vs write from a capability key. Write verbs
// (send/write/manage/publish/upload/post/create/delete/provision) push actions
// out to the provider; everything else reads data in. Centralised so the whole
// catalog stays consistent without hand-annotating ~60 capabilities.
func capabilityDirection(key string) string {
	k := strings.ToLower(key)
	writeMarkers := []string{
		".write", ".send", ".manage", ".post", ".publish", ".upload",
		".create", ".delete", ".update", "provisioning.write", "directory.write",
	}
	for _, m := range writeMarkers {
		if strings.Contains(k, m) {
			return "write"
		}
	}
	// bare write-intent keys without a dotted suffix
	switch k {
	case "publishing", "actions", "write", "send", "manage":
		return "write"
	}
	return "read"
}

// withCapabilityDirections stamps Direction on every capability of every
// provider (idempotent: an explicitly-set direction is preserved). Applied at
// the catalog boundary so all callers — /api/v1/providers, OAuth catalog,
// readiness — emit consistent direction metadata.
func withCapabilityDirections(catalog []Provider) []Provider {
	for pi := range catalog {
		caps := catalog[pi].Capabilities
		for ci := range caps {
			if strings.TrimSpace(caps[ci].Direction) == "" {
				caps[ci].Direction = capabilityDirection(caps[ci].Key)
			}
		}
	}
	return catalog
}

type Bundle struct {
	Key          string   `json:"key"`
	Label        string   `json:"label"`
	Description  string   `json:"description"`
	Capabilities []string `json:"capabilities"`
}

type Provider struct {
	Key              string       `json:"key"`
	Label            string       `json:"label"`
	Category         string       `json:"category"`
	ConnectorType    string       `json:"connectorType"`
	AuthType         string       `json:"authType"`
	DirectOAuthReady bool         `json:"directOAuthReady"`
	Configured       bool         `json:"configured"`
	Status           string       `json:"status"`
	MissingConfig    []string     `json:"missingConfig,omitempty"`
	Capabilities     []Capability `json:"capabilities"`
	Bundles          []Bundle     `json:"bundles"`
	MetaSDK          *MetaSDK     `json:"metaSdk,omitempty"`
	// SupersededBy names the provider that replaces this one in the catalog UI
	// (e.g. facebook/instagram/whatsapp/meta-ads → "meta"). Superseded
	// providers stay in the catalog so existing connections keep resolving,
	// but new-connection UIs should render only the superseding provider.
	SupersededBy string `json:"supersededBy,omitempty"`
}

type MetaSDK struct {
	Enabled       bool   `json:"enabled"`
	AppID         string `json:"appId,omitempty"`
	APIVersion    string `json:"apiVersion,omitempty"`
	Locale        string `json:"locale,omitempty"`
	LoginConfigID string `json:"loginConfigId,omitempty"`
}

func Catalog() []Provider {
	return withCapabilityDirections([]Provider{
		Microsoft(),
		GoogleWorkspace(),
		Slack(),
		Discord(),
		GitHub(),
		Notion(),
		Shopify(),
		Stripe(),
		LinkedIn(),
		X(),
		Meta(),
		Instagram(),
		Facebook(),
		WhatsApp(),
		MetaAds(),
		TikTok(),
		Snapchat(),
		Shipping(),
		Okta(),
		SCIM(),
	})
}

func WithReadiness(catalog []Provider, readiness map[string][]string) []Provider {
	out := make([]Provider, 0, len(catalog))
	for _, provider := range catalog {
		missing := append([]string{}, readiness[provider.Key]...)
		provider.MissingConfig = missing
		if len(missing) == 0 {
			provider.Configured = true
			provider.Status = "ready"
		} else {
			provider.Configured = false
			provider.Status = "missing_config"
		}
		if len(missing) == 0 && !provider.DirectOAuthReady && provider.AuthType != "inbound_scim" {
			provider.Status = "manual_or_admin_config"
		}
		out = append(out, provider)
	}
	return out
}

func OAuthCatalog() []Provider {
	return withCapabilityDirections([]Provider{
		Microsoft(),
		Slack(),
		Discord(),
		GoogleWorkspace(),
		Notion(),
		GitHub(),
		Shopify(),
		Stripe(),
		LinkedIn(),
		X(),
		Meta(),
		Instagram(),
		Facebook(),
		WhatsApp(),
		MetaAds(),
		TikTok(),
		Snapchat(),
	})
}

func Find(key string) (Provider, bool) {
	normalized := NormalizeKey(key)
	for _, provider := range Catalog() {
		if provider.Key == normalized || provider.ConnectorType == normalized {
			return provider, true
		}
	}
	return Provider{}, false
}

func FindOAuth(key string) (Provider, bool) {
	normalized := NormalizeKey(key)
	for _, provider := range OAuthCatalog() {
		if provider.Key == normalized || provider.ConnectorType == normalized {
			return provider, true
		}
	}
	return Provider{}, false
}

func Microsoft() Provider {
	return Provider{
		Key:              "microsoft",
		Label:            "Microsoft 365",
		Category:         "productivity",
		ConnectorType:    "microsoft-graph",
		AuthType:         "oauth2_authorization_code_pkce",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "profile.read",
				Label:       "Basic profile",
				Description: "Identify the connecting Microsoft user and tenant.",
				Scopes:      []string{"openid", "profile", "email", "offline_access", "User.Read"},
			},
			{
				Key:         "sharepoint.read",
				Label:       "SharePoint and OneDrive read",
				Description: "Read sites, drives, permissions, and file metadata for knowledge indexing.",
				Scopes:      []string{"Files.Read.All", "Sites.Read.All"},
			},
			{
				Key:         "sharepoint.write",
				Label:       "SharePoint and OneDrive write",
				Description: "Move, archive, or delete files after human approval.",
				Scopes:      []string{"Files.ReadWrite.All", "Sites.ReadWrite.All"},
				Sensitive:   true,
			},
			{
				Key:         "teams.read",
				Label:       "Teams metadata",
				Description: "Read joined teams and channel metadata.",
				Scopes:      []string{"Team.ReadBasic.All", "Channel.ReadBasic.All"},
			},
			{
				Key:         "teams.messages.read",
				Label:       "Teams messages",
				Description: "Read Teams channel messages and chats for the unified inbox (delegated Graph delta sync).",
				Scopes:      []string{"ChannelMessage.Read.All", "Chat.Read"},
				Sensitive:   true,
			},
			{
				Key:         "mail.read",
				Label:       "Outlook read",
				Description: "Read mailbox messages for shared inbox and AI drafts.",
				Scopes:      []string{"Mail.Read"},
				Sensitive:   true,
			},
			{
				Key:         "mail.send",
				Label:       "Outlook send",
				Description: "Send replies only after Verevon workflow and human-in-the-loop policy allows it.",
				Scopes:      []string{"Mail.Send"},
				Sensitive:   true,
			},
			{
				Key:         "calendar.read",
				Label:       "Calendar read",
				Description: "Read calendars for scheduling context.",
				Scopes:      []string{"Calendars.Read"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Safe onboarding preview",
				Description:  "Identity plus bounded Microsoft 365 metadata.",
				Capabilities: []string{"profile.read", "sharepoint.read", "teams.read"},
			},
			{
				Key:          "knowledge",
				Label:        "Knowledge sync",
				Description:  "SharePoint and OneDrive read access for Data Plane ingestion.",
				Capabilities: []string{"profile.read", "sharepoint.read", "teams.read"},
			},
			{
				Key:          "inbox",
				Label:        "Inbox automation",
				Description:  "Read and send Outlook mail plus Teams messages through Verevon-controlled workflows.",
				Capabilities: []string{"profile.read", "mail.read", "mail.send", "teams.read", "teams.messages.read"},
			},
			{
				Key:          "full",
				Label:        "Full Microsoft workspace",
				Description:  "Knowledge, Teams metadata and messages, Outlook, and calendar capabilities.",
				Capabilities: []string{"profile.read", "sharepoint.read", "teams.read", "teams.messages.read", "mail.read", "mail.send", "calendar.read"},
			},
		},
	}
}

func Slack() Provider {
	return Provider{
		Key:              "slack",
		Label:            "Slack",
		Category:         "chat",
		ConnectorType:    "slack",
		AuthType:         "oauth2_authorization_code",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "workspace.read",
				Label:       "Workspace metadata",
				Description: "Read the Slack workspace name and installation metadata.",
				Scopes:      []string{"team:read"},
			},
			{
				Key:         "users.read",
				Label:       "Member directory metadata",
				Description: "Read basic member profile metadata for routing and attribution.",
				Scopes:      []string{"users:read"},
			},
			{
				Key:         "channels.read",
				Label:       "Public channel metadata",
				Description: "Read visible public channel names and IDs for knowledge source selection.",
				Scopes:      []string{"channels:read"},
			},
			{
				Key:         "channels.history",
				Label:       "Public channel messages",
				Description: "Read public channel message history for knowledge ingestion.",
				Scopes:      []string{"channels:history"},
				Sensitive:   true,
			},
			{
				Key:         "private_channels.read",
				Label:       "Private channel metadata",
				Description: "Read private channel metadata only after explicit admin consent.",
				Scopes:      []string{"groups:read"},
				Sensitive:   true,
			},
			{
				Key:         "files.read",
				Label:       "Shared files",
				Description: "Read Slack file metadata and contents selected for knowledge ingestion.",
				Scopes:      []string{"files:read"},
				Sensitive:   true,
			},
			{
				Key:         "messages.read",
				Label:       "Direct message history",
				Description: "Read bot direct messages and group DMs for the unified inbox.",
				Scopes:      []string{"im:read", "im:history", "mpim:read", "mpim:history"},
				Sensitive:   true,
			},
			{
				Key:         "messages.write",
				Label:       "Post messages",
				Description: "Post Verevon-approved replies or workflow messages.",
				Scopes:      []string{"chat:write"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Safe onboarding preview",
				Description:  "Workspace, member, and public channel metadata only.",
				Capabilities: []string{"workspace.read", "users.read", "channels.read"},
			},
			{
				Key:          "inbox",
				Label:        "Unified inbox",
				Description:  "Channel history plus bot DMs for the shared inbox.",
				Capabilities: []string{"workspace.read", "users.read", "channels.read", "channels.history", "messages.read"},
			},
			{
				Key:          "knowledge",
				Label:        "Knowledge sync",
				Description:  "Public channel history and files selected for knowledge ingestion.",
				Capabilities: []string{"workspace.read", "users.read", "channels.read", "channels.history", "files.read"},
			},
			{
				Key:          "actions",
				Label:        "Slack actions",
				Description:  "Post workflow messages after human-in-the-loop approval.",
				Capabilities: []string{"workspace.read", "users.read", "messages.write"},
			},
			{
				Key:          "full",
				Label:        "Full Slack workspace",
				Description:  "Knowledge sync, unified inbox, approved message actions, and private metadata consent.",
				Capabilities: []string{"workspace.read", "users.read", "channels.read", "channels.history", "private_channels.read", "files.read", "messages.read", "messages.write"},
			},
		},
	}
}

func GoogleWorkspace() Provider {
	return Provider{
		Key:              "google",
		Label:            "Google Workspace",
		Category:         "productivity",
		ConnectorType:    "google-workspace",
		AuthType:         "oauth2_authorization_code_pkce",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "profile.read",
				Label:       "Basic profile",
				Description: "Identify the connecting Google user and workspace.",
				Scopes:      []string{"openid", "email", "profile"},
			},
			{
				Key:         "drive.metadata",
				Label:       "Drive metadata",
				Description: "Read Drive file and folder metadata for safe source discovery.",
				Scopes:      []string{"https://www.googleapis.com/auth/drive.metadata.readonly"},
			},
			{
				Key:         "drive.read",
				Label:       "Drive read",
				Description: "Read selected Drive documents for knowledge ingestion.",
				Scopes:      []string{"https://www.googleapis.com/auth/drive.readonly"},
				Sensitive:   true,
			},
			{
				Key:         "gmail.read",
				Label:       "Gmail read",
				Description: "Read mailbox messages for inbox triage and AI drafts.",
				Scopes:      []string{"https://www.googleapis.com/auth/gmail.readonly"},
				Sensitive:   true,
			},
			{
				Key:         "gmail.send",
				Label:       "Gmail send",
				Description: "Send replies only after Verevon workflow and human-in-the-loop policy allows it.",
				Scopes:      []string{"https://www.googleapis.com/auth/gmail.send"},
				Sensitive:   true,
			},
			{
				Key:         "calendar.read",
				Label:       "Calendar read",
				Description: "Read calendars for scheduling context.",
				Scopes:      []string{"https://www.googleapis.com/auth/calendar.readonly"},
				Sensitive:   true,
			},
			{
				// The adwords scope has no readonly variant — consent wording is
				// full-management. Google Ads API calls additionally require a
				// developer token (GOOGLE_ADS_DEVELOPER_TOKEN header), which is
				// provisioned separately from OAuth in a Google Ads manager account.
				Key:         "ads.manage",
				Label:       "Google Ads",
				Description: "Manage and report on Google Ads campaigns for connected accounts.",
				Scopes:      []string{"https://www.googleapis.com/auth/adwords"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Safe onboarding preview",
				Description:  "Profile and Drive metadata only.",
				Capabilities: []string{"profile.read", "drive.metadata"},
			},
			{
				Key:          "ads",
				Label:        "Google advertising",
				Description:  "Google Ads campaign management and reporting.",
				Capabilities: []string{"profile.read", "ads.manage"},
			},
			{
				Key:          "knowledge",
				Label:        "Knowledge sync",
				Description:  "Read selected Google Drive knowledge sources.",
				Capabilities: []string{"profile.read", "drive.read"},
			},
			{
				Key:          "inbox",
				Label:        "Inbox automation",
				Description:  "Read and send Gmail through Verevon-controlled workflows.",
				Capabilities: []string{"profile.read", "gmail.read", "gmail.send"},
			},
			{
				Key:          "full",
				Label:        "Full Google Workspace",
				Description:  "Drive, Gmail, and calendar capabilities.",
				Capabilities: []string{"profile.read", "drive.read", "gmail.read", "gmail.send", "calendar.read"},
			},
		},
	}
}

func Notion() Provider {
	return Provider{
		Key:              "notion",
		Label:            "Notion",
		Category:         "documents",
		ConnectorType:    "notion",
		AuthType:         "oauth2_authorization_code",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "workspace.read",
				Label:       "Workspace metadata",
				Description: "Read workspace identity and selected top-level pages/databases.",
			},
			{
				Key:         "content.read",
				Label:       "Page and database read",
				Description: "Read selected pages and databases for knowledge ingestion.",
				Sensitive:   true,
			},
			{
				Key:         "content.write",
				Label:       "Page and database write",
				Description: "Create or update Notion content after explicit approval.",
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Safe onboarding preview",
				Description:  "Workspace metadata and selected top-level source labels.",
				Capabilities: []string{"workspace.read"},
			},
			{
				Key:          "knowledge",
				Label:        "Knowledge sync",
				Description:  "Read selected pages and databases.",
				Capabilities: []string{"workspace.read", "content.read"},
			},
			{
				Key:          "full",
				Label:        "Full Notion workspace",
				Description:  "Knowledge sync plus approved write actions.",
				Capabilities: []string{"workspace.read", "content.read", "content.write"},
			},
		},
	}
}

func GitHub() Provider {
	return Provider{
		Key:              "github",
		Label:            "GitHub",
		Category:         "developer",
		ConnectorType:    "github",
		AuthType:         "oauth2_authorization_code",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "profile.read",
				Label:       "Basic profile",
				Description: "Read the connecting GitHub user and email metadata.",
				Scopes:      []string{"read:user", "user:email"},
			},
			{
				Key:         "org.read",
				Label:       "Organization metadata",
				Description: "Read visible organization membership metadata.",
				Scopes:      []string{"read:org"},
			},
			{
				Key:         "repo.public.read",
				Label:       "Public repository read",
				Description: "Read public repository metadata, README files, issues, and wiki availability.",
				Scopes:      []string{"public_repo"},
			},
			{
				Key:         "repo.contents.read",
				Label:       "Repository contents",
				Description: "Read repository contents and README files selected for knowledge ingestion.",
				Scopes:      []string{"public_repo"},
				Sensitive:   true,
			},
			{
				Key:         "commits.read",
				Label:       "Commit history",
				Description: "Read repository commit metadata for source traceability.",
				Scopes:      []string{"public_repo"},
				Sensitive:   true,
			},
			{
				Key:         "pulls.read",
				Label:       "Pull requests",
				Description: "Read pull request metadata and review status for engineering context.",
				Scopes:      []string{"public_repo"},
				Sensitive:   true,
			},
			{
				Key:         "issues.read",
				Label:       "Issues read",
				Description: "Read issue metadata and discussion context selected for support or planning workflows.",
				Scopes:      []string{"public_repo"},
				Sensitive:   true,
			},
			{
				Key:         "repo.private.read",
				Label:       "Private repository read",
				Description: "Read private repositories selected for knowledge ingestion.",
				Scopes:      []string{"repo"},
				Sensitive:   true,
			},
			{
				Key:         "issues.write",
				Label:       "Issue actions",
				Description: "Create or update issues after workflow approval.",
				Scopes:      []string{"repo"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Safe onboarding preview",
				Description:  "Profile, organization, and public repository metadata.",
				Capabilities: []string{"profile.read", "org.read", "repo.public.read"},
			},
			{
				Key:          "knowledge",
				Label:        "Knowledge sync",
				Description:  "Read repository metadata and selected repository content.",
				Capabilities: []string{"profile.read", "org.read", "repo.public.read", "repo.contents.read", "commits.read", "pulls.read", "issues.read", "repo.private.read"},
			},
			{
				Key:          "full",
				Label:        "Full GitHub workspace",
				Description:  "Knowledge sync plus approved issue actions.",
				Capabilities: []string{"profile.read", "org.read", "repo.public.read", "repo.contents.read", "commits.read", "pulls.read", "issues.read", "repo.private.read", "issues.write"},
			},
		},
	}
}

func Shopify() Provider {
	return Provider{
		Key:              "shopify",
		Label:            "Shopify",
		Category:         "commerce",
		ConnectorType:    "shopify",
		AuthType:         "oauth2_authorization_code_shop",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "store.read",
				Label:       "Store metadata",
				Description: "Read shop identity, locale, currency, and storefront metadata.",
			},
			{
				Key:         "products.read",
				Label:       "Products read",
				Description: "Read product catalog details for ecommerce support answers.",
				Scopes:      []string{"read_products"},
			},
			{
				Key:         "content.read",
				Label:       "Content read",
				Description: "Read shop pages, blogs, and policies for support knowledge.",
				Scopes:      []string{"read_content"},
			},
			{
				Key:         "orders.read",
				Label:       "Orders read",
				Description: "Read order status for authenticated customer support workflows.",
				Scopes:      []string{"read_orders"},
				Sensitive:   true,
			},
			{
				Key:         "customers.read",
				Label:       "Customers read",
				Description: "Read customer records for authenticated support workflows.",
				Scopes:      []string{"read_customers"},
				Sensitive:   true,
			},
			{
				Key:         "orders.write",
				Label:       "Order actions",
				Description: "Create order-side support actions only through approved workflows.",
				Scopes:      []string{"write_orders"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Safe onboarding preview",
				Description:  "Store, product, content, and policy metadata.",
				Capabilities: []string{"store.read", "products.read", "content.read"},
			},
			{
				Key:          "commerce",
				Label:        "Commerce support",
				Description:  "Read orders and customers for authenticated support.",
				Capabilities: []string{"store.read", "products.read", "content.read", "orders.read", "customers.read"},
			},
			{
				Key:          "full",
				Label:        "Full Shopify support",
				Description:  "Commerce support plus approved order actions.",
				Capabilities: []string{"store.read", "products.read", "content.read", "orders.read", "customers.read", "orders.write"},
			},
		},
	}
}

func Stripe() Provider {
	return Provider{
		Key:              "stripe",
		Label:            "Stripe",
		Category:         "billing",
		ConnectorType:    "stripe",
		AuthType:         "oauth2_authorization_code",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "account.read",
				Label:       "Account metadata",
				Description: "Read Stripe account identity and business metadata.",
				Scopes:      []string{"read_only"},
			},
			{
				Key:         "customers.read",
				Label:       "Customers read",
				Description: "Read customer records for support context.",
				Scopes:      []string{"read_only"},
				Sensitive:   true,
			},
			{
				Key:         "billing.read",
				Label:       "Billing read",
				Description: "Read subscriptions, invoices, and payment status.",
				Scopes:      []string{"read_only"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Safe onboarding preview",
				Description:  "Account identity and broad billing availability.",
				Capabilities: []string{"account.read"},
			},
			{
				Key:          "billing",
				Label:        "Billing support",
				Description:  "Customer, subscription, and invoice context for support.",
				Capabilities: []string{"account.read", "customers.read", "billing.read"},
			},
		},
	}
}

func LinkedIn() Provider {
	return Provider{
		Key:              "linkedin",
		Label:            "LinkedIn",
		Category:         "social",
		ConnectorType:    "linkedin",
		AuthType:         "oauth2_authorization_code_app_review",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "social.profile.read",
				Label:       "Member identity",
				Description: "Read the connecting member identity through Sign in with LinkedIn using OpenID Connect.",
				Scopes:      []string{"openid", "profile", "email"},
			},
			{
				Key:         "social.profile.verify",
				Label:       "Basic verified profile",
				Description: "Read the member's basic profile information required by LinkedIn verification flows.",
				Scopes:      []string{"r_profile_basicinfo"},
				Sensitive:   true,
			},
			{
				Key:         "social.verification.read",
				Label:       "Profile verification",
				Description: "Read the member's profile verification status where Verified on LinkedIn access is approved.",
				Scopes:      []string{"r_verify"},
				Sensitive:   true,
			},
			{
				Key:         "social.verification.details.read",
				Label:       "Profile verification details",
				Description: "Read detailed verification report data for approved Verified on LinkedIn integrations.",
				Scopes:      []string{"r_verify_details"},
				Sensitive:   true,
			},
			{
				Key:         "social.organization.read",
				Label:       "Organization pages",
				Description: "Read organization/page ACLs and page publishing targets for the connected member.",
				Scopes:      []string{"r_organization_social"},
				Sensitive:   true,
			},
			{
				Key:         "social.organization.write",
				Label:       "Organization page actions",
				Description: "Create or update organization page content after workflow approval.",
				Scopes:      []string{"w_organization_social"},
				Sensitive:   true,
			},
			{
				Key:         "social.post.read",
				Label:       "Posts read",
				Description: "Read member or organization social posts where LinkedIn product access permits it.",
				Scopes:      []string{"r_member_social", "r_organization_social"},
				Sensitive:   true,
			},
			{
				Key:         "social.post.write",
				Label:       "Create posts",
				Description: "Publish approved posts to LinkedIn profiles or organization pages.",
				Scopes:      []string{"w_member_social"},
				Sensitive:   true,
			},
			{
				Key:         "social.media.upload",
				Label:       "Upload media",
				Description: "Upload approved media assets used by scheduled LinkedIn posts.",
				Scopes:      []string{"w_member_social", "w_organization_social"},
				Sensitive:   true,
			},
			{
				Key:         "social.events.manage",
				Label:       "Event management",
				Description: "Create and manage LinkedIn events for approved organization/member workflows.",
				Scopes:      []string{"r_organization_social", "w_organization_social"},
				Sensitive:   true,
			},
			{
				Key:         "social.ads.read",
				Label:       "Ads reporting",
				Description: "Read LinkedIn Campaign Manager accounts and reporting metadata.",
				Scopes:      []string{"r_ads"},
				Sensitive:   true,
			},
			{
				Key:         "social.ads.manage",
				Label:       "Ads management",
				Description: "Create or modify LinkedIn ads assets and campaign entities after approval.",
				Scopes:      []string{"rw_ads"},
				Sensitive:   true,
			},
			{
				Key:         "social.conversions.manage",
				Label:       "Conversions API",
				Description: "Create and manage Conversions API rules and event sources for Campaign Manager accounts.",
				Scopes:      []string{"r_ads", "rw_conversions"},
				Sensitive:   true,
			},
			{
				Key:         "social.leads.read",
				Label:       "Lead sync",
				Description: "Read lead gen forms and lead form responses for approved advertisers.",
				Scopes:      []string{"r_marketing_leadgen_automation"},
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Post analytics",
				Description: "Read post status, reach, engagement, and error details.",
				Scopes:      []string{"r_organization_social", "r_ads"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Social account preview",
				Description:  "Profile and page metadata only.",
				Capabilities: []string{"social.profile.read"},
			},
			{
				Key:          "verification",
				Label:        "Profile verification",
				Description:  "Basic profile information plus profile verification status.",
				Capabilities: []string{"social.profile.read", "social.profile.verify", "social.verification.read"},
			},
			{
				Key:          "verification_details",
				Label:        "Verification details",
				Description:  "Detailed Verified on LinkedIn report data for approved integrations.",
				Capabilities: []string{"social.profile.read", "social.profile.verify", "social.verification.read", "social.verification.details.read"},
			},
			{
				Key:          "publishing",
				Label:        "Approved publishing",
				Description:  "Schedule and publish approved LinkedIn posts with media.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload"},
			},
			{
				Key:          "organization",
				Label:        "Organization pages",
				Description:  "Organization page targets plus approved organization content actions.",
				Capabilities: []string{"social.profile.read", "social.organization.read", "social.organization.write"},
			},
			{
				Key:          "ads",
				Label:        "LinkedIn Ads",
				Description:  "Campaign Manager account reporting and approved management operations.",
				Capabilities: []string{"social.profile.read", "social.ads.read", "social.ads.manage", "social.analytics.read"},
			},
			{
				Key:          "conversions",
				Label:        "Conversions API",
				Description:  "LinkedIn Ads conversion configuration and event-source management.",
				Capabilities: []string{"social.profile.read", "social.ads.read", "social.conversions.manage"},
			},
			{
				Key:          "lead_sync",
				Label:        "Lead Sync",
				Description:  "Lead gen form and response sync for approved advertisers.",
				Capabilities: []string{"social.profile.read", "social.leads.read"},
			},
			{
				Key:          "events",
				Label:        "Event management",
				Description:  "Create and manage LinkedIn events for approved organization/member workflows.",
				Capabilities: []string{"social.profile.read", "social.organization.read", "social.events.manage"},
			},
			{
				Key:          "full",
				Label:        "Full LinkedIn suite",
				Description:  "Identity, verification, publishing, organizations, events, ads, conversions, lead sync, and analytics.",
				Capabilities: []string{"social.profile.read", "social.profile.verify", "social.verification.read", "social.post.read", "social.post.write", "social.media.upload", "social.organization.read", "social.organization.write", "social.events.manage", "social.ads.read", "social.ads.manage", "social.conversions.manage", "social.leads.read", "social.analytics.read"},
			},
		},
	}
}

func X() Provider {
	return Provider{
		Key:              "x",
		Label:            "X",
		Category:         "social",
		ConnectorType:    "x",
		AuthType:         "oauth2_authorization_code_pkce_app_review",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "social.profile.read",
				Label:       "Profile metadata",
				Description: "Read account identity and available posting context.",
				Scopes:      []string{"tweet.read", "users.read", "offline.access"},
			},
			{
				Key:         "social.post.write",
				Label:       "Create posts",
				Description: "Publish approved posts and threads.",
				Scopes:      []string{"tweet.write"},
				Sensitive:   true,
			},
			{
				Key:         "social.media.upload",
				Label:       "Upload media",
				Description: "Upload approved images or video for scheduled posts.",
				Scopes:      []string{"tweet.write"},
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Post analytics",
				Description: "Read status and engagement metadata for published posts.",
				Sensitive:   true,
			},
			{
				Key:         "social.inbox.read",
				Label:       "Direct messages",
				Description: "Read account direct messages for the unified inbox (requires X API Pro tier or above).",
				Scopes:      []string{"dm.read"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Social account preview",
				Description:  "Account metadata only.",
				Capabilities: []string{"social.profile.read"},
			},
			{
				Key:          "publishing",
				Label:        "Approved publishing",
				Description:  "Schedule and publish approved X posts.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload"},
			},
			{
				Key:          "inbox",
				Label:        "Unified inbox",
				Description:  "Direct messages for the shared inbox.",
				Capabilities: []string{"social.profile.read", "social.inbox.read"},
			},
			{
				Key:          "full",
				Label:        "Publishing, inbox, and analytics",
				Description:  "Publishing, direct messages, and performance reporting.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload", "social.analytics.read", "social.inbox.read"},
			},
		},
	}
}

// Meta is the unified Meta integration: ONE OAuth connect covering Facebook
// Pages, Instagram (professional accounts linked to a Page), WhatsApp
// Business, and Meta Ads (Marketing API). It supersedes the separate
// facebook / instagram / whatsapp / meta-ads providers, which remain in the
// catalog only so existing connections keep resolving.
//
// The instagram_* scopes are the "Instagram API with Facebook Login" flavor
// (instagram_basic is NOT deprecated for that product — what died was the
// standalone Basic Display API). With META_BUSINESS_LOGIN_CONFIG_ID set, the
// dialog uses Facebook Login for Business and the configuration decides the
// granted permissions; the scope lists below then serve as capability
// documentation and the classic-login fallback.
func Meta() Provider {
	return Provider{
		Key:              "meta",
		Label:            "Meta",
		Category:         "social",
		ConnectorType:    "meta",
		AuthType:         "meta_oauth_app_review",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "social.messenger.read",
				Label:       "Messenger Page access",
				Description: "List Facebook Pages the operator manages and read the Page metadata needed to select a Messenger inbox.",
				Scopes:      []string{"pages_show_list", "pages_read_engagement"},
				Sensitive:   true,
			},
			{
				Key:         "social.profile.read",
				Label:       "Business & Page metadata",
				Description: "Read the connected Meta business, Facebook Pages, and linked account identity.",
				Scopes:      []string{"pages_show_list", "pages_read_engagement", "business_management"},
			},
			{
				Key:         "social.instagram.read",
				Label:       "Instagram account",
				Description: "Read the linked Instagram professional account profile, media, and insights.",
				Scopes:      []string{"instagram_basic", "instagram_manage_insights"},
			},
			{
				Key:         "social.post.write",
				Label:       "Publish to Facebook & Instagram",
				Description: "Publish approved posts to connected Facebook Pages and Instagram professional accounts.",
				Scopes:      []string{"pages_manage_posts", "instagram_content_publish"},
				Sensitive:   true,
			},
			{
				Key:         "social.media.upload",
				Label:       "Upload media",
				Description: "Prepare and submit approved media for Page and Instagram publishing.",
				Scopes:      []string{"pages_manage_posts", "instagram_content_publish"},
				Sensitive:   true,
			},
			{
				Key:         "social.inbox.read",
				Label:       "Page & Instagram conversations",
				Description: "Read Page comments plus Instagram professional-account conversations for unified inbox workflows.",
				Scopes:      []string{"pages_read_user_content", "pages_manage_metadata", "instagram_basic", "instagram_manage_messages"},
				Sensitive:   true,
			},
			{
				Key:         "social.messenger.manage",
				Label:       "Messenger",
				Description: "Send approved Messenger replies and subscribe connected Pages to webhook fields.",
				Scopes:      []string{"pages_messaging", "pages_manage_metadata"},
				Sensitive:   true,
			},
			{
				Key:         "social.whatsapp.manage",
				Label:       "WhatsApp Business",
				Description: "Manage WhatsApp Business accounts and send approved template/session messages via the Cloud API.",
				Scopes:      []string{"whatsapp_business_management", "whatsapp_business_messaging"},
				Sensitive:   true,
			},
			{
				Key:         "social.ads.manage",
				Label:       "Meta Ads management",
				Description: "Manage Meta ad accounts, campaigns, ad sets, creatives, and delivery status.",
				Scopes:      []string{"ads_management", "business_management"},
				Sensitive:   true,
			},
			{
				Key:         "social.catalog.manage",
				Label:       "Commerce catalogs",
				Description: "Create, read, update, and batch-manage Meta commerce and Advantage+ catalog items.",
				Scopes:      []string{"catalog_management", "business_management"},
				Sensitive:   true,
			},
			{
				Key:         "social.threads.manage",
				Label:       "Threads",
				Description: "Read Threads profile metadata and publish approved Threads posts.",
				Scopes:      []string{"threads_basic", "threads_content_publish"},
				Sensitive:   true,
			},
			{
				Key:         "social.oembed.read",
				Label:       "oEmbed",
				Description: "Resolve Facebook, Instagram, and Threads oEmbed metadata for approved URLs.",
			},
			{
				Key:         "social.live.manage",
				Label:       "Live video",
				Description: "Create and schedule approved Facebook Page live video broadcasts.",
				Scopes:      []string{"pages_manage_posts", "pages_read_engagement"},
				Sensitive:   true,
			},
			{
				Key:         "social.app_ads.manage",
				Label:       "App install ads",
				Description: "Create and manage app-install campaigns through the Meta Marketing API.",
				Scopes:      []string{"ads_management", "business_management"},
				Sensitive:   true,
			},
			{
				Key:         "social.audience_network.read",
				Label:       "Audience Network",
				Description: "Read Audience Network business and placement metadata where provider access permits it.",
				Scopes:      []string{"business_management", "ads_read"},
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Pages, Instagram & ads analytics",
				Description: "Read Page, Instagram, and ad campaign performance metadata.",
				Scopes:      []string{"read_insights", "ads_read", "instagram_manage_insights", "threads_manage_insights"},
				Sensitive:   true,
			},
			{
				Key:         "social.conversions.manage",
				Label:       "Conversions API",
				Description: "Submit server-side conversion events for a Meta ad account or dataset.",
				// Meta's Conversions API has no scopes of its own — it's
				// authorized via the same Marketing API permissions as
				// social.ads.manage. Kept as a distinct capability (matching
				// this catalog's LinkedIn Conversions API entry) so a
				// narrower, partner-scoped Business Login configuration can
				// be selected for it instead of the general connection flow's
				// configuration — see the "conversions" bundle below.
				Scopes:    []string{"ads_management", "business_management"},
				Sensitive: true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Business preview",
				Description:  "Business, Page, and account metadata only.",
				Capabilities: []string{"social.profile.read"},
			},
			{
				Key:          "publishing",
				Label:        "Approved publishing",
				Description:  "Publish approved posts to Facebook Pages and Instagram.",
				Capabilities: []string{"social.profile.read", "social.instagram.read", "social.post.write", "social.media.upload"},
			},
			{
				Key:          "messenger",
				Label:        "Messenger inbox",
				Description:  "Connect Facebook Pages for Messenger conversations and approved replies only.",
				Capabilities: []string{"social.messenger.read", "social.messenger.manage"},
			},
			{
				Key:          "inbox",
				Label:        "Unified inbox",
				Description:  "Page conversations plus WhatsApp Business messaging.",
				Capabilities: []string{"social.profile.read", "social.inbox.read", "social.messenger.manage", "social.whatsapp.manage"},
			},
			{
				Key:          "ads",
				Label:        "Meta advertising",
				Description:  "Campaign, app-install, catalog, Audience Network, and ads reporting workflows.",
				Capabilities: []string{"social.profile.read", "social.ads.manage", "social.app_ads.manage", "social.catalog.manage", "social.audience_network.read", "social.analytics.read"},
			},
			{
				Key:          "conversions",
				Label:        "Conversions API partner integration",
				Description:  "Server-side conversion event submission for a partner Conversions API integration — uses its own Business Login configuration, separate from the general connection flow.",
				Capabilities: []string{"social.profile.read", "social.conversions.manage"},
			},
			{
				Key:          "commerce",
				Label:        "Catalog commerce",
				Description:  "Commerce catalog management for shops and Advantage+ catalog ads.",
				Capabilities: []string{"social.profile.read", "social.catalog.manage"},
			},
			{
				Key:          "threads",
				Label:        "Threads publishing",
				Description:  "Threads profile access and approved publishing.",
				Capabilities: []string{"social.profile.read", "social.threads.manage", "social.analytics.read"},
			},
			{
				Key:          "embed",
				Label:        "Embeds",
				Description:  "Resolve Facebook, Instagram, and Threads oEmbed metadata.",
				Capabilities: []string{"social.oembed.read"},
			},
			{
				Key:          "live",
				Label:        "Live video",
				Description:  "Create and schedule approved Page live broadcasts.",
				Capabilities: []string{"social.profile.read", "social.live.manage"},
			},
			{
				Key:          "full",
				Label:        "Full Meta suite",
				Description:  "Pages, Instagram, WhatsApp, Messenger, ads, catalogs, Threads, embeds, live video, and analytics in one connection.",
				Capabilities: []string{"social.profile.read", "social.instagram.read", "social.post.write", "social.media.upload", "social.inbox.read", "social.messenger.manage", "social.whatsapp.manage", "social.ads.manage", "social.catalog.manage", "social.threads.manage", "social.oembed.read", "social.live.manage", "social.app_ads.manage", "social.audience_network.read", "social.analytics.read"},
			},
		},
	}
}

// Shipping is Verevon's OWN freight aggregator (shipping-core in the Ingestion
// Plane): one integration covers the whole carrier fleet — Bring, PostNord,
// DHL, DSV, Helthjem, Porterbuddy, m.fl. — the nShift/Logistra Cargonizer
// model, in-house. There is no per-user OAuth: carrier credentials are
// org/admin-level (Mybring API key, UPS/FedEx OAuth apps) configured on
// shipping-core itself, so this provider is catalog-visible with an
// admin-setup status rather than a connect popup. Quote comparison works out
// of the box on the demo fleet; carriers flip to live agreement prices as
// their credentials are configured.
func Shipping() Provider {
	return Provider{
		Key:              "shipping",
		Label:            "Frakt & sporing",
		Category:         "shipping",
		ConnectorType:    "shipping",
		AuthType:         "aggregator_admin_config",
		DirectOAuthReady: false,
		Capabilities: []Capability{
			{
				Key:         "shipping.quotes.read",
				Label:       "Fraktpriser",
				Description: "Sammenlign priser og leveringstid fra hele transportørflåten i sanntid.",
			},
			{
				Key:         "shipping.carriers.read",
				Label:       "Transportører",
				Description: "Se hvilke transportører som er tilgjengelige og om de kjører avtalepriser.",
			},
			{
				Key:         "shipping.tracking.read",
				Label:       "Sporing",
				Description: "Følg sendinger med sporingsnummer (Bring i dag, flere transportører senere).",
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Fraktsammenligning",
				Description:  "Priser, transportører og sporing.",
				Capabilities: []string{"shipping.quotes.read", "shipping.carriers.read", "shipping.tracking.read"},
			},
			{
				Key:          "full",
				Label:        "Full frakt",
				Description:  "Alle frakt-kapabiliteter (booking kommer).",
				Capabilities: []string{"shipping.quotes.read", "shipping.carriers.read", "shipping.tracking.read"},
			},
		},
	}
}

// Discord covers workspace (guild) identity intake via plain OAuth2. Reading
// or sending channel messages requires a bot with privileged intents — NOT
// offered here; the OAuth `messages.read` scope is an RPC-client scope and
// does not grant REST access to server messages.
func Discord() Provider {
	return Provider{
		Key:              "discord",
		Label:            "Discord",
		Category:         "chat",
		ConnectorType:    "discord",
		AuthType:         "oauth2_authorization_code",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "profile.read",
				Label:       "User identity",
				Description: "Identify the connecting Discord user (username and email).",
				Scopes:      []string{"identify", "email"},
			},
			{
				Key:         "workspace.read",
				Label:       "Server list",
				Description: "Read the servers (guilds) the connecting user belongs to.",
				Scopes:      []string{"guilds"},
			},
			{
				Key:         "messages.read",
				Label:       "Server messages",
				Description: "Install the Verevon bot into the selected server so channel messages reach the unified inbox via the Gateway (requires the Message Content privileged intent on the Discord app).",
				Scopes:      []string{"bot"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Identity preview",
				Description:  "User identity only.",
				Capabilities: []string{"profile.read"},
			},
			{
				Key:          "inbox",
				Label:        "Unified inbox",
				Description:  "Bot install for server message ingestion.",
				Capabilities: []string{"profile.read", "workspace.read", "messages.read"},
			},
			{
				Key:          "full",
				Label:        "Identity, servers, and inbox",
				Description:  "User identity, server membership, and bot message ingestion.",
				Capabilities: []string{"profile.read", "workspace.read", "messages.read"},
			},
		},
	}
}

func Instagram() Provider {
	return Provider{
		Key:              "instagram",
		Label:            "Instagram",
		Category:         "social",
		ConnectorType:    "instagram",
		AuthType:         "meta_oauth_app_review",
		DirectOAuthReady: true,
		SupersededBy:     "meta",
		Capabilities: []Capability{
			{
				Key:         "social.inbox.read",
				Label:       "Instagram conversations",
				Description: "Read conversations and comments for a professional account through Instagram Login.",
				Scopes:      []string{"instagram_business_basic", "instagram_business_manage_comments", "instagram_business_manage_messages"},
				Sensitive:   true,
			},
			{
				Key:         "social.messenger.manage",
				Label:       "Instagram replies",
				Description: "Send approved Instagram replies through Instagram Login.",
				Scopes:      []string{"instagram_business_manage_messages"},
				Sensitive:   true,
			},
			{
				Key:         "social.profile.read",
				Label:       "Business profile metadata",
				Description: "Read the connected Instagram professional account identity.",
				Scopes:      []string{"instagram_business_basic"},
			},
			{
				Key:         "social.post.write",
				Label:       "Create posts",
				Description: "Publish approved feed posts, reels, and carousel posts through provider-specific media workflows.",
				Scopes:      []string{"instagram_content_publish"},
				Sensitive:   true,
			},
			{
				Key:         "social.media.upload",
				Label:       "Upload media",
				Description: "Prepare and submit approved media containers for publishing.",
				Scopes:      []string{"instagram_content_publish"},
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Post analytics",
				Description: "Read post status, reach, and engagement metadata.",
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "inbox",
				Label:        "Instagram inbox",
				Description:  "Read and reply to linked Instagram professional-account conversations.",
				Capabilities: []string{"social.inbox.read", "social.messenger.manage"},
			},
			{
				Key:          "onboarding",
				Label:        "Social account preview",
				Description:  "Business account metadata only.",
				Capabilities: []string{"social.profile.read"},
			},
			{
				Key:          "publishing",
				Label:        "Approved publishing",
				Description:  "Schedule and publish approved Instagram posts with media preparation.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload"},
			},
			{
				Key:          "full",
				Label:        "Publishing and analytics",
				Description:  "Publishing plus performance reporting.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload", "social.analytics.read"},
			},
		},
	}
}

func Facebook() Provider {
	return Provider{
		Key:              "facebook",
		Label:            "Facebook",
		Category:         "social",
		ConnectorType:    "facebook",
		AuthType:         "meta_oauth_app_review",
		DirectOAuthReady: true,
		SupersededBy:     "meta",
		Capabilities: []Capability{
			{
				Key:         "social.profile.read",
				Label:       "Page metadata",
				Description: "Read connected Facebook Page identity and publishing readiness.",
				Scopes:      []string{"pages_show_list", "pages_read_engagement"},
			},
			{
				Key:         "social.post.write",
				Label:       "Create Page posts",
				Description: "Publish approved text, link, or photo posts to connected Facebook Pages.",
				Scopes:      []string{"pages_manage_posts"},
				Sensitive:   true,
			},
			{
				Key:         "social.media.upload",
				Label:       "Upload media",
				Description: "Prepare approved image assets for Page publishing.",
				Scopes:      []string{"pages_manage_posts"},
				Sensitive:   true,
			},
			{
				Key:         "social.inbox.read",
				Label:       "Page conversations",
				Description: "Read Page comments and conversation metadata for unified inbox workflows.",
				Scopes:      []string{"pages_read_user_content", "pages_manage_metadata"},
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Page analytics",
				Description: "Read Page and post performance metadata.",
				Scopes:      []string{"read_insights"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Social account preview",
				Description:  "Page metadata only.",
				Capabilities: []string{"social.profile.read"},
			},
			{
				Key:          "publishing",
				Label:        "Approved Page publishing",
				Description:  "Schedule and publish approved Facebook Page posts.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload"},
			},
			{
				Key:          "inbox",
				Label:        "Page inbox",
				Description:  "Page metadata plus comment/conversation intake for unified inbox workflows.",
				Capabilities: []string{"social.profile.read", "social.inbox.read"},
			},
			{
				Key:          "full",
				Label:        "Publishing, inbox, and analytics",
				Description:  "Publishing, inbox intake, and performance reporting.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload", "social.inbox.read", "social.analytics.read"},
			},
		},
	}
}

func WhatsApp() Provider {
	return Provider{
		Key:              "whatsapp",
		Label:            "WhatsApp Business",
		Category:         "social",
		ConnectorType:    "whatsapp",
		AuthType:         "meta_oauth_app_review",
		DirectOAuthReady: true,
		SupersededBy:     "meta",
		Capabilities: []Capability{
			{
				Key:         "social.profile.read",
				Label:       "Business account metadata",
				Description: "Read connected WhatsApp Business account identity and phone number readiness.",
				Scopes:      []string{"whatsapp_business_management", "business_management"},
			},
			{
				Key:         "social.inbox.read",
				Label:       "Customer conversations",
				Description: "Read webhook-backed WhatsApp conversation metadata for unified inbox workflows.",
				Scopes:      []string{"whatsapp_business_messaging"},
				Sensitive:   true,
			},
			{
				Key:         "social.post.write",
				Label:       "Approved messaging",
				Description: "Send approved WhatsApp Business template and session messages.",
				Scopes:      []string{"whatsapp_business_messaging"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Business account preview",
				Description:  "Business account metadata only.",
				Capabilities: []string{"social.profile.read"},
			},
			{
				Key:          "inbox",
				Label:        "WhatsApp inbox",
				Description:  "Business account metadata plus approved inbox messaging workflows.",
				Capabilities: []string{"social.profile.read", "social.inbox.read", "social.post.write"},
			},
			{
				Key:          "full",
				Label:        "WhatsApp Business messaging",
				Description:  "Business account, inbox, and approved messaging workflows.",
				Capabilities: []string{"social.profile.read", "social.inbox.read", "social.post.write"},
			},
		},
	}
}

func MetaAds() Provider {
	return Provider{
		Key:              "meta-ads",
		Label:            "Meta Ads",
		Category:         "social",
		ConnectorType:    "meta-ads",
		AuthType:         "meta_oauth_app_review",
		DirectOAuthReady: true,
		SupersededBy:     "meta",
		Capabilities: []Capability{
			{
				Key:         "social.profile.read",
				Label:       "Business metadata",
				Description: "Read connected Meta business and ad account identity.",
				Scopes:      []string{"business_management"},
			},
			{
				Key:         "social.ads.manage",
				Label:       "Campaign management",
				Description: "Manage Meta ad accounts, campaigns, ad sets, creatives, and delivery status.",
				Scopes:      []string{"ads_management", "business_management"},
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Ads analytics",
				Description: "Read campaign, ad set, creative, and account reporting metadata.",
				Scopes:      []string{"ads_read", "read_insights"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Ad account preview",
				Description:  "Business and ad account metadata only.",
				Capabilities: []string{"social.profile.read"},
			},
			{
				Key:          "ads",
				Label:        "Meta advertising",
				Description:  "Campaign management and reporting workflows.",
				Capabilities: []string{"social.profile.read", "social.ads.manage", "social.analytics.read"},
			},
			{
				Key:          "full",
				Label:        "Campaign management and analytics",
				Description:  "Meta Ads management plus reporting.",
				Capabilities: []string{"social.profile.read", "social.ads.manage", "social.analytics.read"},
			},
		},
	}
}

func TikTok() Provider {
	return Provider{
		Key:           "tiktok",
		Label:         "TikTok",
		Category:      "social",
		ConnectorType: "tiktok",
		AuthType:      "oauth2_authorization_code_app_review",
		// Login Kit v2 flow is implemented (client_key naming); readiness still
		// gates on TIKTOK_CLIENT_KEY/SECRET. Web apps require a public https
		// redirect URI — localhost only works for Desktop-type TikTok apps.
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "social.profile.read",
				Label:       "Creator profile metadata",
				Description: "Read connected TikTok creator identity and content posting readiness.",
				Scopes:      []string{"user.info.basic"},
			},
			{
				Key:         "social.post.write",
				Label:       "Create posts",
				Description: "Publish approved direct posts through TikTok's content posting workflow.",
				Scopes:      []string{"video.publish"},
				Sensitive:   true,
			},
			{
				Key:         "social.media.upload",
				Label:       "Upload media",
				Description: "Prepare approved video or image media for scheduled posts.",
				Scopes:      []string{"video.upload"},
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Post analytics",
				Description: "Read post status and performance metadata where provider access permits it.",
				Scopes:      []string{"video.list", "user.info.stats"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Social account preview",
				Description:  "Creator profile metadata only.",
				Capabilities: []string{"social.profile.read"},
			},
			{
				Key:          "publishing",
				Label:        "Approved publishing",
				Description:  "Schedule and publish approved TikTok posts with media preparation.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload"},
			},
			{
				Key:          "full",
				Label:        "Publishing and analytics",
				Description:  "Publishing plus performance reporting.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload", "social.analytics.read"},
			},
		},
	}
}

func Snapchat() Provider {
	return Provider{
		Key:              "snapchat",
		Label:            "Snapchat",
		Category:         "social",
		ConnectorType:    "snapchat",
		AuthType:         "oauth2_authorization_code_app_review",
		DirectOAuthReady: true,
		Capabilities: []Capability{
			{
				Key:         "social.profile.read",
				Label:       "Public profile metadata",
				Description: "Read allowlisted Snapchat Public Profile metadata and content (stories, spotlights, saved stories) where available.",
				Scopes:      []string{"snapchat-profile-api"},
			},
			{
				Key:   "social.post.write",
				Label: "Organic content publishing",
				// Public Profile API (businessapi.snapchat.com) Content Management.
				// ALLOWLIST-ONLY: Snap must allowlist the OAuth app's client id and
				// the connecting user needs a Partnership Role on the target
				// profile. social-core keeps live posting behind SNAPCHAT_LIVE_PUBLISHING.
				Description: "Post Stories, Spotlights, and Saved Stories to a Snapchat Public Profile (allowlist-gated).",
				Scopes:      []string{"snapchat-marketing-api"},
				Sensitive:   true,
			},
			{
				Key:         "social.media.upload",
				Label:       "Upload media",
				Description: "Prepare video/image media containers for Snapchat Story, Spotlight, and ad-creative posting.",
				Scopes:      []string{"snapchat-marketing-api"},
				Sensitive:   true,
			},
			{
				Key:         "social.ads.manage",
				Label:       "Marketing API",
				Description: "Manage Snapchat organizations, ad accounts, campaigns, creatives, and reporting through the Marketing API.",
				Scopes:      []string{"snapchat-marketing-api"},
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Ads analytics",
				Description: "Read Snapchat campaign and ad account performance metadata.",
				Scopes:      []string{"snapchat-marketing-api"},
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Social account preview",
				Description:  "Profile and organization metadata only.",
				Capabilities: []string{"social.profile.read"},
			},
			{
				Key:          "publishing",
				Label:        "Approved publishing",
				Description:  "Post approved organic Stories/Spotlights to a Public Profile (allowlist-gated).",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload"},
			},
			{
				Key:          "ads",
				Label:        "Snapchat marketing",
				Description:  "Ad account, creative, media, and reporting workflows.",
				Capabilities: []string{"social.profile.read", "social.media.upload", "social.ads.manage", "social.analytics.read"},
			},
			{
				Key:          "full",
				Label:        "Publishing, marketing, and analytics",
				Description:  "Organic publishing plus Snapchat marketing workflows and reporting.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload", "social.ads.manage", "social.analytics.read"},
			},
		},
	}
}

func Okta() Provider {
	return Provider{
		Key:              "okta",
		Label:            "Okta",
		Category:         "identity",
		ConnectorType:    "okta",
		AuthType:         "api_token_or_oauth2_admin",
		DirectOAuthReady: false,
		Capabilities: []Capability{
			{
				Key:         "tenant.read",
				Label:       "Tenant metadata",
				Description: "Read Okta org identity and high-level integration readiness.",
			},
			{
				Key:         "directory.read",
				Label:       "Directory read",
				Description: "Read users, groups, and lifecycle metadata for identity context.",
				Sensitive:   true,
			},
			{
				Key:         "directory.write",
				Label:       "Directory actions",
				Description: "Create, update, suspend, or activate users after admin approval.",
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Identity preview",
				Description:  "High-level Okta tenant availability only.",
				Capabilities: []string{"tenant.read"},
			},
			{
				Key:          "full",
				Label:        "Identity administration",
				Description:  "Directory read plus approved lifecycle actions.",
				Capabilities: []string{"directory.read", "directory.write"},
			},
		},
	}
}

func SCIM() Provider {
	return Provider{
		Key:              "scim",
		Label:            "SCIM",
		Category:         "identity",
		ConnectorType:    "scim",
		AuthType:         "inbound_scim",
		DirectOAuthReady: false,
		Capabilities: []Capability{
			{
				Key:         "provisioning.read",
				Label:       "Provisioning read",
				Description: "Receive user and group provisioning state.",
				Sensitive:   true,
			},
			{
				Key:         "provisioning.write",
				Label:       "Provisioning updates",
				Description: "Receive create, update, deactivate, and group membership changes.",
				Sensitive:   true,
			},
		},
		Bundles: []Bundle{
			{
				Key:          "full",
				Label:        "SCIM provisioning",
				Description:  "Inbound identity provisioning for enterprise workspaces.",
				Capabilities: []string{"provisioning.read", "provisioning.write"},
			},
		},
	}
}

func NormalizeKey(key string) string {
	normalized := strings.TrimSpace(strings.ToLower(key))
	switch normalized {
	case "m365", "microsoft365", "microsoft-365", "teams", "sharepoint", "onedrive", "outlook", "microsoft-graph":
		return "microsoft"
	case "google-drive", "gdrive", "drive", "gmail", "google-workspace":
		return "google"
	case "git", "github-oauth":
		return "github"
	case "stripe-connect":
		return "stripe"
	case "linkedin-pages", "linkedin-page", "linkedin-organization":
		return "linkedin"
	case "twitter", "twitter-x", "x-twitter":
		return "x"
	case "ig", "instagram-business", "meta-instagram":
		return "instagram"
	case "facebook-page", "facebook-pages", "meta-facebook":
		return "facebook"
	case "whatsapp-business", "whatsapp-cloud", "whatsapp-business-platform", "meta-whatsapp":
		return "whatsapp"
	case "meta-business", "meta-suite", "facebook-business", "meta-unified":
		return "meta"
	case "facebook-ads", "facebook-marketing", "metaads", "meta_ads", "meta-marketing", "meta-marketing-api", "ads-manager":
		return "meta-ads"
	case "tik-tok", "tiktok-business":
		return "tiktok"
	case "snap", "snapchat-ads", "snapchat-marketing":
		return "snapchat"
	case "okta-oauth", "okta-api":
		return "okta"
	case "scim-v2", "scim2":
		return "scim"
	default:
		return normalized
	}
}

func ResolveCapabilities(provider Provider, requestedCapabilities, requestedBundles []string) []string {
	selected := map[string]struct{}{}
	bundles := map[string]Bundle{}
	for _, bundle := range provider.Bundles {
		bundles[bundle.Key] = bundle
	}
	for _, requested := range requestedBundles {
		if bundle, ok := bundles[strings.TrimSpace(requested)]; ok {
			for _, capability := range bundle.Capabilities {
				selected[capability] = struct{}{}
			}
		}
	}
	for _, capability := range requestedCapabilities {
		capability = strings.TrimSpace(capability)
		if capability != "" {
			selected[capability] = struct{}{}
		}
	}
	if len(selected) == 0 {
		for _, bundle := range provider.Bundles {
			if bundle.Key == "onboarding" {
				for _, capability := range bundle.Capabilities {
					selected[capability] = struct{}{}
				}
			}
		}
	}
	return sortedKeys(selected)
}

func ResolveScopes(provider Provider, capabilities []string) []string {
	capabilityMap := map[string]Capability{}
	for _, capability := range provider.Capabilities {
		capabilityMap[capability.Key] = capability
	}
	scopes := map[string]struct{}{}
	for _, key := range capabilities {
		capability, ok := capabilityMap[key]
		if !ok {
			continue
		}
		for _, scope := range capability.Scopes {
			scopes[scope] = struct{}{}
		}
	}
	return sortedKeys(scopes)
}

func sortedKeys(values map[string]struct{}) []string {
	out := make([]string, 0, len(values))
	for key := range values {
		out = append(out, key)
	}
	sort.Strings(out)
	return out
}
