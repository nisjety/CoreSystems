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
}

func Catalog() []Provider {
	return []Provider{
		Microsoft(),
		GoogleWorkspace(),
		Slack(),
		GitHub(),
		Notion(),
		Shopify(),
		Stripe(),
		LinkedIn(),
		X(),
		Instagram(),
		Facebook(),
		TikTok(),
		Snapchat(),
		Okta(),
		SCIM(),
	}
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
	return []Provider{
		Microsoft(),
		Slack(),
		GoogleWorkspace(),
		Notion(),
		GitHub(),
		Shopify(),
		Stripe(),
		LinkedIn(),
		X(),
		Instagram(),
		Facebook(),
		Snapchat(),
	}
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
				Key:         "mail.read",
				Label:       "Outlook read",
				Description: "Read mailbox messages for shared inbox and AI drafts.",
				Scopes:      []string{"Mail.Read"},
				Sensitive:   true,
			},
			{
				Key:         "mail.send",
				Label:       "Outlook send",
				Description: "Send replies only after Velion workflow and human-in-the-loop policy allows it.",
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
				Description:  "Read and send Outlook mail through Velion-controlled workflows.",
				Capabilities: []string{"profile.read", "mail.read", "mail.send"},
			},
			{
				Key:          "full",
				Label:        "Full Microsoft workspace",
				Description:  "Knowledge, Teams metadata, Outlook, and calendar capabilities.",
				Capabilities: []string{"profile.read", "sharepoint.read", "teams.read", "mail.read", "mail.send", "calendar.read"},
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
				Key:         "messages.write",
				Label:       "Post messages",
				Description: "Post Velion-approved replies or workflow messages.",
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
				Description:  "Knowledge sync plus approved message actions and private metadata consent.",
				Capabilities: []string{"workspace.read", "users.read", "channels.read", "channels.history", "private_channels.read", "files.read", "messages.write"},
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
				Description: "Send replies only after Velion workflow and human-in-the-loop policy allows it.",
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
		},
		Bundles: []Bundle{
			{
				Key:          "onboarding",
				Label:        "Safe onboarding preview",
				Description:  "Profile and Drive metadata only.",
				Capabilities: []string{"profile.read", "drive.metadata"},
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
				Description:  "Read and send Gmail through Velion-controlled workflows.",
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
				Capabilities: []string{"profile.read", "org.read", "repo.public.read", "repo.private.read"},
			},
			{
				Key:          "full",
				Label:        "Full GitHub workspace",
				Description:  "Knowledge sync plus approved issue actions.",
				Capabilities: []string{"profile.read", "org.read", "repo.public.read", "repo.private.read", "issues.write"},
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
				Label:       "Profile and page metadata",
				Description: "Read member identity and available organization/page publishing targets.",
				Scopes:      []string{"openid", "profile", "email"},
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
				Scopes:      []string{"w_member_social"},
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Post analytics",
				Description: "Read post status, reach, engagement, and error details.",
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
				Key:          "publishing",
				Label:        "Approved publishing",
				Description:  "Schedule and publish approved LinkedIn posts with media.",
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
				Key:          "full",
				Label:        "Publishing and analytics",
				Description:  "Publishing plus performance reporting.",
				Capabilities: []string{"social.profile.read", "social.post.write", "social.media.upload", "social.analytics.read"},
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
		Capabilities: []Capability{
			{
				Key:         "social.profile.read",
				Label:       "Business profile metadata",
				Description: "Read connected Instagram business account identity and publishing readiness.",
				Scopes:      []string{"instagram_basic", "pages_show_list", "pages_read_engagement"},
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

func TikTok() Provider {
	return Provider{
		Key:              "tiktok",
		Label:            "TikTok",
		Category:         "social",
		ConnectorType:    "tiktok",
		AuthType:         "oauth2_authorization_code_app_review",
		DirectOAuthReady: false,
		Capabilities: []Capability{
			{
				Key:         "social.profile.read",
				Label:       "Creator profile metadata",
				Description: "Read connected TikTok creator identity and content posting readiness.",
			},
			{
				Key:         "social.post.write",
				Label:       "Create posts",
				Description: "Publish approved direct posts through TikTok's content posting workflow.",
				Sensitive:   true,
			},
			{
				Key:         "social.media.upload",
				Label:       "Upload media",
				Description: "Prepare approved video or image media for scheduled posts.",
				Sensitive:   true,
			},
			{
				Key:         "social.analytics.read",
				Label:       "Post analytics",
				Description: "Read post status and performance metadata where provider access permits it.",
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
				Description: "Read allowlisted Snapchat Public Profile metadata where available.",
				Scopes:      []string{"snapchat-profile-api"},
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
				Key:          "ads",
				Label:        "Snapchat marketing",
				Description:  "Ad account, campaign, creative, and reporting workflows.",
				Capabilities: []string{"social.profile.read", "social.ads.manage", "social.analytics.read"},
			},
			{
				Key:          "full",
				Label:        "Marketing and analytics",
				Description:  "Snapchat marketing workflows and reporting.",
				Capabilities: []string{"social.profile.read", "social.ads.manage", "social.analytics.read"},
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
