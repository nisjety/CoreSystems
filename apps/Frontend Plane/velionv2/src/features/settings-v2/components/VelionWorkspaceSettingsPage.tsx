"use client";

import { Check, ChevronDown, MoreHorizontal } from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type ReactNode,
  type SetStateAction,
} from "react";
import {
  VerevonButton,
  VerevonIconButton,
  VerevonInput,
  VerevonSelect,
  VerevonSwitch,
} from "@/components/ui/verevon-ui";
import { cn } from "@/lib/utils";
import {
  useEntitlements,
  useOrgId,
} from "@/features/shell-v2/lib/control-plane-provider";
import type { ControlPlaneEntitlements } from "@/lib/control-plane/context-types";
import {
  workspaceSettingsSectionIds,
  workspaceSettingsSections,
  getWorkspaceSettingsSection,
  isWorkspaceSettingsSection,
  type WorkspaceSettingsSectionId,
  type SectionDetail,
} from "@/features/settings-v2/lib/settings-sections";
import { apiGet, apiSend } from "@/lib/api/client-envelope";

export type { WorkspaceSettingsSectionId };
export { workspaceSettingsSectionIds, workspaceSettingsSections, isWorkspaceSettingsSection, getWorkspaceSettingsSection };

const sectionDetails = Object.fromEntries(workspaceSettingsSections.map((section) => [section.id, section])) as Record<
  WorkspaceSettingsSectionId,
  SectionDetail
>;

type StatusCard = { label: string; value: string; detail: string; tone?: "ok" | "warn" | "neutral" };

type LiveMember = { userId: string; name?: string; email: string; role: string; status: string };

// memberRows removed — members are fetched live from /api/org/orgs/{orgId}/members

type IntegrationSettingsProvider = {
  key: string;
  label: string;
  category: string;
  configured: boolean;
  status: string;
  missingConfig: string[];
  directOAuthReady: boolean;
  capabilities: Array<{ key: string; sensitive?: boolean }>;
};

type IntegrationSettingsConnection = {
  id: string;
  providerKey: string;
  providerLabel: string;
  displayName: string;
  status: string;
  capabilities: string[];
  scopeCount: number;
  syncStatus: string;
  latestSyncJob?: { status: string; updatedAt?: string };
};

type IntegrationSettingsSummary = {
  metrics: {
    connected: number;
    failed: number;
    readyProviders: number;
    syncing: number;
    totalProviders: number;
  };
  providers: IntegrationSettingsProvider[];
  connections: IntegrationSettingsConnection[];
};

type ConnectSessionResult = {
  authMode?: "direct-oauth";
  connectUrl: string;
  expiresAt?: string;
  providerConfigKey?: string;
  sessionToken?: string;
};

type IntegrationSettingsRow = {
  action: "admin" | "connect" | "loading" | "missing" | "connected";
  connection?: IntegrationSettingsConnection;
  detail: string;
  name: string;
  provider: IntegrationSettingsProvider;
  status: string;
};

const fallbackIntegrationRows = [
  { name: "Microsoft 365", detail: "SharePoint, OneDrive, Outlook, Teams", status: "Loading", action: "loading" as const },
  { name: "Google Workspace", detail: "Drive, Gmail, Calendar", status: "Loading", action: "loading" as const },
  { name: "Slack", detail: "Workspace metadata and channels", status: "Loading", action: "loading" as const },
  { name: "GitHub", detail: "Repositories, README, issues", status: "Loading", action: "loading" as const },
];

const securityToggles = [
  {
    title: "Require MFA for admins",
    description: "Admins must use multi-factor authentication before accessing organization settings.",
    enabled: true,
  },
  {
    title: "Restrict sign-in to verified domains",
    description: "Only users with approved workspace domains can sign in.",
    enabled: true,
  },
  {
    title: "Log admin configuration changes",
    description: "Keep an audit trail for billing, member, SSO, and integration changes.",
    enabled: true,
  },
];

const sectionStatusCards: Record<
  WorkspaceSettingsSectionId,
  Array<{ label: string; value: string; detail: string; tone?: "ok" | "warn" | "neutral" }>
> = {
  workspace: [
    { label: "Primary domain", value: "Verified", detail: "aquatiq.no is ready for customer-facing links.", tone: "ok" },
    { label: "Data region", value: "Europe", detail: "All new workspace data is stored in EU infrastructure.", tone: "neutral" },
    { label: "Routing owner", value: "Support ops", detail: "Default inbox ownership is assigned.", tone: "ok" },
  ],
  // members and billing status cards are rendered by their respective live-data sections
  members: [],
  billing: [],
  sso: [
    { label: "Provider", value: "Google", detail: "Metadata loaded from Google Workspace.", tone: "ok" },
    { label: "SCIM", value: "Ready", detail: "Provisioning token generated but not enforced.", tone: "neutral" },
    { label: "Enforcement", value: "Admins only", detail: "Members can still sign in with email.", tone: "warn" },
  ],
  "org-security": [
    { label: "MFA", value: "Required", detail: "Admins must use MFA for sensitive settings.", tone: "ok" },
    { label: "Sessions", value: "30 days", detail: "Idle sessions are revoked after 30 days.", tone: "neutral" },
    { label: "Audit log", value: "365 days", detail: "Security and billing changes are retained.", tone: "ok" },
  ],
  integrations: [
    { label: "Connected apps", value: "2 / 4", detail: "Zendesk and Slack are connected.", tone: "neutral" },
    { label: "Sync health", value: "Healthy", detail: "Last workspace sync finished 8 minutes ago.", tone: "ok" },
    { label: "Webhook errors", value: "0", detail: "No failed deliveries in the last 24 hours.", tone: "ok" },
  ],
};

const domainRows = [
  { domain: "aquatiq.no", status: "Verified", owner: "Customer portal" },
  { domain: "support.aquatiq.no", status: "DNS pending", owner: "Help center" },
];

const businessHourRows = [
  { day: "Monday-Friday", hours: "08:00-17:00", inbox: "Priority support" },
  { day: "Saturday", hours: "10:00-14:00", inbox: "Overflow" },
];

const roleRows = [
  { role: "Owner", access: "Full workspace, billing, and security", members: "1" },
  { role: "Admin", access: "Members, inboxes, automations, and integrations", members: "1" },
  { role: "Agent", access: "Assigned conversations and knowledge suggestions", members: "1" },
  { role: "Viewer", access: "Reports and read-only customer context", members: "0" },
];

// invoiceRows removed — no invoice-list endpoint exists; replaced with empty state

const ssoMappingRows = [
  { attribute: "email", source: "primaryEmail", destination: "User email" },
  { attribute: "department", source: "orgUnitPath", destination: "Team" },
  { attribute: "role", source: "customSchema.verevonRole", destination: "Workspace role" },
];

type AuditEvent = {
  id?: string;
  action?: string;
  actor?: string;
  outcome?: string;
  resource?: string;
  ipAddress?: string;
  requestId?: string;
  createdAt?: string;
};

const webhookRows = [
  { endpoint: "Zendesk ticket sync", status: "200 OK", lastRun: "8 min ago" },
  { endpoint: "Slack escalation", status: "200 OK", lastRun: "14 min ago" },
  { endpoint: "CRM customer upsert", status: "Paused", lastRun: "2 days ago" },
];

export function VerevonWorkspaceSettingsPage({
  section = "workspace",
}: {
  section?: WorkspaceSettingsSectionId;
}) {
  const details = sectionDetails[section];
  const entitlements = useEntitlements();
  const orgId = useOrgId();

  // Build live status cards for billing and members; fall back to static cards
  // for sections that have no live data source yet.
  const billingStatusCards: StatusCard[] = [
    {
      label: "Current plan",
      value: entitlements?.plan ?? "—",
      detail: entitlements?.subscriptionStatus
        ? `Status: ${entitlements.subscriptionStatus}`
        : "Loading plan information…",
      tone: "neutral",
    },
    {
      label: "Credits",
      value: entitlements != null ? String(entitlements.credits) : "—",
      detail: "Available credits on this plan",
      tone: "neutral",
    },
    {
      label: "Payment method",
      value: "—",
      detail: "Manage payment in billing settings.",
      tone: "neutral",
    },
  ];

  const liveStatusCards: Record<WorkspaceSettingsSectionId, StatusCard[]> = {
    ...sectionStatusCards,
    billing: billingStatusCards,
    // members status grid is rendered inline by MembersSection (needs async count)
    members: sectionStatusCards.members,
  };

  return (
    <div className="relative h-full overflow-y-auto bg-[#F2F2F1] text-[#111111] [scrollbar-gutter:stable] dark:bg-[#111214] dark:text-[#F7F8F8]">
      <div
        aria-hidden="true"
        className="pointer-events-none sticky top-0 z-20 h-14 bg-gradient-to-b from-[#F2F2F1] via-[#F2F2F1]/88 to-transparent backdrop-blur-[2px] dark:from-[#111214] dark:via-[#111214]/88"
      />

      <div className="-mt-8 mx-auto w-full max-w-[960px] px-5 pb-40 pt-7 lg:px-8">
        <main className="min-w-0">
          <div className="mb-10">
            <p className="mb-4 text-[11px] font-semibold uppercase tracking-[0.22em] text-[#9A9CA3]">Admin</p>
            <h1 className="text-[36px] font-semibold leading-tight tracking-normal text-[#111111] dark:text-white sm:text-[40px]">
              {details.title}
            </h1>
            <p className="mt-3 max-w-[680px] text-[13px] leading-5 text-[#656A73] dark:text-[#A9ADB6]">
              {details.description}
            </p>
          </div>

          {liveStatusCards[section].length > 0 && (
            <StatusGrid cards={liveStatusCards[section]} />
          )}

          <section id={details.id} className="mt-10 scroll-mt-24">
            <WorkspaceSettingsSection section={section} orgId={orgId} entitlements={entitlements} />
          </section>

          <div className="relative z-30 mt-10 flex items-center justify-end gap-3 border-t border-[#E8E8EA] pt-6 dark:border-white/10">
            <VerevonButton className="px-5">
              Cancel
            </VerevonButton>
            <VerevonButton variant="primary" className="px-5">
              <Check className="size-4" strokeWidth={1.8} />
              {details.saveLabel}
            </VerevonButton>
          </div>
        </main>
      </div>

      <div
        aria-hidden="true"
        className="pointer-events-none sticky bottom-0 z-20 h-14 bg-gradient-to-t from-[#F2F2F1] via-[#F2F2F1]/82 to-transparent dark:from-[#111214] dark:via-[#111214]/82"
      />
    </div>
  );
}

function WorkspaceSettingsSection({
  section,
  orgId,
  entitlements,
}: {
  section: WorkspaceSettingsSectionId;
  orgId: string | null;
  entitlements: ControlPlaneEntitlements | null;
}) {
  switch (section) {
    case "members":
      return <MembersSection orgId={orgId} />;
    case "billing":
      return <BillingSection entitlements={entitlements} />;
    case "sso":
      return <SsoSection />;
    case "org-security":
      return <OrgSecuritySection />;
    case "integrations":
      return <IntegrationsSection />;
    case "workspace":
    default:
      return <WorkspaceSection />;
  }
}

function StatusGrid({
  cards,
}: {
  cards: Array<{ label: string; value: string; detail: string; tone?: "ok" | "warn" | "neutral" }>;
}) {
  return (
    <div className="grid gap-3 sm:grid-cols-3">
      {cards.map((card) => (
        <div key={card.label} className="rounded-[18px] border border-[#E1E2E4] bg-white/42 p-4 dark:border-white/10 dark:bg-white/5">
          <div className="flex items-center gap-2">
            <span
              className={cn(
                "size-2 rounded-full",
                card.tone === "ok"
                  ? "bg-[#16A34A]"
                  : card.tone === "warn"
                    ? "bg-[#DD7A1F]"
                    : "bg-[#9CA3AF]",
              )}
              aria-hidden="true"
            />
            <p className="text-[10px] font-semibold uppercase tracking-[0.16em] text-[#8F939C]">{card.label}</p>
          </div>
          <p className="mt-3 text-[20px] font-semibold leading-tight text-[#111111] dark:text-white">{card.value}</p>
          <p className="mt-1 text-[12px] leading-5 text-[#737780] dark:text-[#A9ADB6]">{card.detail}</p>
        </div>
      ))}
    </div>
  );
}

function WorkspaceSection() {
  return (
    <>
      <SectionHeader title="Workspace basics" description="Shared workspace fields that affect URLs, defaults, and support routing." />
      <div className="grid gap-6 sm:grid-cols-2">
        <SettingsField id="workspace-name" label="Workspace name" defaultValue="aquatiq-as" />
        <SettingsField id="workspace-url" label="Workspace URL" defaultValue="aquatiq-as.verevon.ai" />
        <SettingsField id="primary-domain" label="Primary domain" defaultValue="aquatiq.no" />
        <SettingsSelect
          id="data-region"
          label="Data region"
          defaultValue="europe"
          options={[
            { value: "europe", label: "Europe" },
            { value: "us", label: "United States" },
          ]}
        />
        <SettingsSelect
          id="default-language"
          label="Default language"
          defaultValue="english"
          options={[
            { value: "english", label: "English" },
            { value: "norwegian", label: "Norwegian" },
            { value: "french", label: "French" },
          ]}
        />
        <SettingsField id="admin-owner" label="Admin owner" defaultValue="Author Name" />
      </div>
      <div className="mt-8 grid gap-5 lg:grid-cols-2">
        <FeaturePanel
          title="Verified domains"
          description="Domain records ready for DNS verification and customer-facing links."
          actionLabel="Add domain"
        >
          <div className="divide-y divide-[#E8E8EA] dark:divide-white/10">
            {domainRows.map((row) => (
              <DataRow
                key={row.domain}
                primary={row.domain}
                secondary={row.owner}
                meta={row.status}
              />
            ))}
          </div>
        </FeaturePanel>
        <FeaturePanel
          title="Business hours"
          description="Workspace-wide routing windows for support inbox ownership."
          actionLabel="Edit schedule"
        >
          <div className="divide-y divide-[#E8E8EA] dark:divide-white/10">
            {businessHourRows.map((row) => (
              <DataRow
                key={row.day}
                primary={row.day}
                secondary={row.inbox}
                meta={row.hours}
              />
            ))}
          </div>
        </FeaturePanel>
      </div>
    </>
  );
}

function MembersSection({ orgId }: { orgId: string | null }) {
  // When orgId is null (unauthenticated / pre-onboarding), skip the fetch entirely.
  // loading starts true only when we expect a fetch; false immediately when there is no orgId.
  const [members, setMembers] = useState<LiveMember[]>([]);
  const [loading, setLoading] = useState(orgId != null);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    if (!orgId) return;

    const controller = new AbortController();
    abortRef.current = controller;

    fetch(`/api/org/orgs/${orgId}/members`, {
      credentials: "include",
      signal: controller.signal,
    })
      .then((res) => {
        if (!res.ok) throw new Error(`Failed to load members (${res.status})`);
        return res.json() as Promise<{ members: LiveMember[]; count: number }>;
      })
      .then((data) => {
        setMembers(data.members ?? []);
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (err instanceof Error && err.name === "AbortError") return;
        setError("Could not load members.");
        setLoading(false);
      });

    return () => {
      controller.abort();
    };
  }, [orgId]);

  return (
    <>
      <SectionHeader title="Members & roles" description="Invite teammates, assign access, and review seat status." />
      <div className="mb-5 grid gap-3 sm:grid-cols-[minmax(0,1fr)_160px]">
        <SettingsField id="invite-email" label="Invite by email" type="email" placeholder="teammate@company.com" />
        <SettingsSelect
          id="invite-role"
          label="Role"
          defaultValue="agent"
          options={[
            { value: "agent", label: "Agent" },
            { value: "admin", label: "Admin" },
            { value: "owner", label: "Owner" },
          ]}
        />
      </div>
      <div className="overflow-hidden rounded-[18px] border border-[#E1E2E4] bg-white/42 dark:border-white/10 dark:bg-white/5">
        {loading && (
          <p className="px-5 py-6 text-[13px] text-[#737780] dark:text-[#A9ADB6]">Loading members…</p>
        )}
        {!loading && error && (
          <p className="px-5 py-6 text-[13px] text-[#737780] dark:text-[#A9ADB6]">{error}</p>
        )}
        {!loading && !error && members.length === 0 && (
          <p className="px-5 py-6 text-[13px] text-[#737780] dark:text-[#A9ADB6]">No members found.</p>
        )}
        {!loading && !error && members.map((member) => (
          <div key={member.userId} className="grid gap-3 border-b border-[#E8E8EA] px-5 py-4 last:border-b-0 sm:grid-cols-[1fr_120px_96px_32px] sm:items-center dark:border-white/10">
            <div className="min-w-0">
              <p className="truncate text-[13px] font-medium text-[#111111] dark:text-white">{member.name ?? member.email}</p>
              <p className="mt-1 truncate text-[12px] text-[#737780] dark:text-[#A9ADB6]">{member.email}</p>
            </div>
            <span className="text-[13px] font-medium text-[#4D5159] dark:text-[#CACDD4]">{member.role}</span>
            <span className="text-[12px] text-[#737780] dark:text-[#A9ADB6]">{member.status}</span>
            <VerevonIconButton aria-label={`More actions for ${member.name ?? member.email}`} className="text-[#777B84] hover:bg-white dark:hover:bg-white/10">
              <MoreHorizontal className="size-4" strokeWidth={1.7} />
            </VerevonIconButton>
          </div>
        ))}
      </div>
      <FeaturePanel
        title="Role templates"
        description="Reusable access templates for member invites and SSO role mapping."
        actionLabel="Create role"
        className="mt-8"
      >
        <div className="divide-y divide-[#E8E8EA] dark:divide-white/10">
          {roleRows.map((role) => (
            <DataRow
              key={role.role}
              primary={role.role}
              secondary={role.access}
              meta={`${role.members} members`}
            />
          ))}
        </div>
      </FeaturePanel>
    </>
  );
}

function BillingSection({ entitlements }: { entitlements: ControlPlaneEntitlements | null }) {
  const plan = entitlements?.plan ?? "—";
  const status = entitlements?.subscriptionStatus ?? "—";
  const credits = entitlements != null ? String(entitlements.credits) : "—";

  // Derive a seat quota from the entitlements quota map if present
  const seatLimit = entitlements?.quotas?.["seats"] ?? null;
  const seatDisplay = seatLimit != null ? `— / ${seatLimit}` : "—";

  return (
    <>
      <SectionHeader title="Plan & usage" description="Review plan, usage, payment method, and invoices." />
      <div className="grid gap-4 sm:grid-cols-3">
        <Metric label="Plan" value={plan} detail={`Status: ${status}`} />
        <Metric label="Seats" value={seatDisplay} detail={seatLimit != null ? `Up to ${seatLimit} seats on this plan` : "Seat quota unavailable"} />
        <Metric label="Credits" value={credits} detail="Available on this plan" />
      </div>
      <div className="mt-6 grid gap-6 sm:grid-cols-2">
        <SettingsField id="billing-email" label="Billing email" type="email" placeholder="billing@yourcompany.com" />
        <SettingsSelect
          id="usage-cap"
          label="Usage cap"
          defaultValue="notify"
          options={[
            { value: "notify", label: "Notify at 80%" },
            { value: "pause", label: "Pause at limit" },
            { value: "none", label: "No cap" },
          ]}
        />
      </div>
      <div className="mt-8 grid gap-5 lg:grid-cols-[minmax(0,1fr)_320px]">
        <FeaturePanel
          title="Invoice history"
          description="Workspace invoice history and downloadable billing records."
          actionLabel="Download CSV"
        >
          <p className="py-3 text-[13px] text-[#737780] dark:text-[#A9ADB6]">
            Ingen fakturaer ennå — invoices will appear here once a paid plan is active.
          </p>
        </FeaturePanel>
        <FeaturePanel
          title="Spend controls"
          description="Plan limits and quota details for this workspace."
          actionLabel="Configure"
        >
          {entitlements != null ? (
            <div className="space-y-3 text-[13px] text-[#565B65] dark:text-[#B5BAC4]">
              <p className="flex items-center justify-between gap-4">
                <span>Plan</span>
                <strong className="font-semibold text-[#111111] dark:text-white">{plan}</strong>
              </p>
              <p className="flex items-center justify-between gap-4">
                <span>Status</span>
                <strong className="font-semibold text-[#111111] dark:text-white">{status}</strong>
              </p>
              <p className="flex items-center justify-between gap-4">
                <span>Credits</span>
                <strong className="font-semibold text-[#111111] dark:text-white">{credits}</strong>
              </p>
            </div>
          ) : (
            <p className="py-3 text-[13px] text-[#737780] dark:text-[#A9ADB6]">Loading plan details…</p>
          )}
        </FeaturePanel>
      </div>
    </>
  );
}

function SsoSection() {
  return (
    <>
      <SectionHeader title="SSO configuration" description="Configure organization sign-in, domains, and provisioning." />
      <div className="grid gap-6 sm:grid-cols-2">
        <SettingsSelect
          id="sso-provider"
          label="Provider"
          defaultValue="google"
          options={[
            { value: "google", label: "Google Workspace" },
            { value: "microsoft", label: "Microsoft Entra ID" },
            { value: "saml", label: "SAML 2.0" },
          ]}
        />
        <SettingsField id="sso-domain" label="Allowed domain" defaultValue="aquatiq.no" />
        <SettingsField id="scim-token" label="SCIM token" defaultValue="Configured" />
        <SettingsSelect
          id="sso-enforcement"
          label="Enforcement"
          defaultValue="admins"
          options={[
            { value: "admins", label: "Admins only" },
            { value: "all", label: "All members" },
            { value: "off", label: "Off" },
          ]}
        />
      </div>
      <div className="mt-8 grid gap-5 lg:grid-cols-[320px_minmax(0,1fr)]">
        <FeaturePanel
          title="Connection test"
          description="SSO validation checks to run before enforcing sign-in."
          actionLabel="Run test"
        >
          <div className="space-y-3 text-[13px] text-[#565B65] dark:text-[#B5BAC4]">
            <p className="flex items-center justify-between gap-4"><span>Metadata</span><strong className="font-semibold text-[#111111] dark:text-white">Loaded</strong></p>
            <p className="flex items-center justify-between gap-4"><span>Domain claim</span><strong className="font-semibold text-[#111111] dark:text-white">Verified</strong></p>
            <p className="flex items-center justify-between gap-4"><span>Last test</span><strong className="font-semibold text-[#111111] dark:text-white">2 hours ago</strong></p>
          </div>
        </FeaturePanel>
        <FeaturePanel
          title="Attribute mapping"
          description="SCIM and SAML attributes mapped into Verevon workspace fields."
          actionLabel="Edit mapping"
        >
          <div className="divide-y divide-[#E8E8EA] dark:divide-white/10">
            {ssoMappingRows.map((row) => (
              <DataRow
                key={row.attribute}
                primary={row.attribute}
                secondary={`${row.source} -> ${row.destination}`}
                meta="Mapped"
              />
            ))}
          </div>
        </FeaturePanel>
      </div>
    </>
  );
}

function RecentSecurityEvents() {
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    abortRef.current = controller;

    fetch("/api/v1/audit", {
      credentials: "include",
      signal: controller.signal,
    })
      .then((res) => res.json() as Promise<{ success: boolean; data: AuditEvent[] }>)
      .then((json) => {
        setEvents(Array.isArray(json.data) ? json.data : []);
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (err instanceof Error && err.name === "AbortError") return;
        setEvents([]);
        setLoading(false);
      });

    return () => {
      controller.abort();
    };
  }, []);

  if (loading) {
    return (
      <p className="py-3 text-[13px] text-[#737780] dark:text-[#A9ADB6]">
        Loading security events…
      </p>
    );
  }

  if (events.length === 0) {
    return (
      <p className="py-3 text-[13px] text-[#737780] dark:text-[#A9ADB6]">
        Ingen sikkerhetshendelser ennå
      </p>
    );
  }

  return (
    <div className="divide-y divide-[#E8E8EA] dark:divide-white/10">
      {events.map((event, idx) => {
        const dateStr =
          event.createdAt && !isNaN(new Date(event.createdAt).getTime())
            ? new Date(event.createdAt).toLocaleString()
            : null;
        const secondary = [event.actor, event.ipAddress].filter(Boolean).join(" · ");
        return (
          <DataRow
            key={event.id ?? event.requestId ?? String(idx)}
            primary={`${event.action ?? "Event"}${event.outcome ? ` — ${event.outcome}` : ""}`}
            secondary={secondary || ""}
            meta={dateStr ?? ""}
          />
        );
      })}
    </div>
  );
}

function OrgSecuritySection() {
  return (
    <>
      <SectionHeader title="Security policy" description="Set organization-wide security requirements and audit controls." />
      <div className="divide-y divide-[#E8E8EA] border-y border-[#E8E8EA] dark:divide-white/10 dark:border-white/10">
        {securityToggles.map((toggle) => (
          <ToggleRow key={toggle.title} {...toggle} />
        ))}
      </div>
      <div className="mt-6 grid gap-6 sm:grid-cols-2">
        <SettingsSelect
          id="session-duration"
          label="Session duration"
          defaultValue="30-days"
          options={[
            { value: "7-days", label: "7 days" },
            { value: "30-days", label: "30 days" },
            { value: "90-days", label: "90 days" },
          ]}
        />
        <SettingsField id="audit-retention" label="Audit retention" defaultValue="365 days" />
      </div>
      <FeaturePanel
        title="Recent security events"
        description="Recent organization changes that administrators should review."
        actionLabel="Open audit log"
        className="mt-8"
      >
        <RecentSecurityEvents />
      </FeaturePanel>
    </>
  );
}

function IntegrationsSection() {
  const [summary, setSummary] = useState<IntegrationSettingsSummary | null>(null);
  const [actionBusy, setActionBusy] = useState<string | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [syncProgress, setSyncProgress] = useState<Record<string, string>>({});
  const [notice, setNotice] = useState<string | null>(null);
  const eventSourcesRef = useRef<EventSource[]>([]);

  const refresh = useCallback(() => {
    const controller = new AbortController();
    apiGet<IntegrationSettingsSummary>("/api/v1/integrations", { signal: controller.signal })
      .then((payload) => {
        setSummary(payload);
        setLoadFailed(false);
      })
      .catch((error: unknown) => {
        if (error instanceof DOMException && error.name === "AbortError") return;
        setLoadFailed(true);
      });

    return () => controller.abort();
  }, []);

  useEffect(() => {
    return refresh();
  }, [refresh]);

  useEffect(() => {
    return () => {
      for (const source of eventSourcesRef.current) source.close();
      eventSourcesRef.current = [];
    };
  }, []);

  const rows = buildIntegrationRows(summary);
  const runAction = async (row: IntegrationSettingsRow | (typeof fallbackIntegrationRows)[number], action: "connect" | "disconnect" | "reconnect" | "sync") => {
    if (!("provider" in row)) return;
    const busyKey = `${row.provider.key}:${action}`;
    setActionBusy(busyKey);
    setNotice(null);
    try {
      if (action === "connect") {
        const session = await apiSend<ConnectSessionResult>(
          `/api/v1/integrations/providers/${encodeURIComponent(row.provider.key)}/connect-session`,
          { bundles: ["knowledge"] },
        );
        await runSettingsOAuth(session);
        setNotice(`${row.name} connected.`);
      } else if (action === "reconnect" && row.connection) {
        const session = await apiSend<ConnectSessionResult>(
          `/api/v1/integrations/connections/${encodeURIComponent(row.connection.id)}/reconnect-session`,
          {},
        );
        await runSettingsOAuth(session);
        setNotice(`${row.name} reconnected.`);
      } else if (action === "disconnect" && row.connection) {
        await apiSend<{ disconnected: boolean }>(
          `/api/v1/integrations/connections/${encodeURIComponent(row.connection.id)}`,
          {},
          "DELETE",
        );
        setNotice(`${row.name} disconnected.`);
      } else if (action === "sync" && row.connection) {
        const result = await apiSend<{ syncJob?: { id?: string; status?: string } }>(
          "/api/v1/integrations/sync-jobs",
          { connectionId: row.connection.id, reason: "settings_user_requested", mode: "incremental" },
        );
        const jobId = result.syncJob?.id;
        setSyncProgress((prev) => ({
          ...prev,
          [row.connection!.id]: result.syncJob?.status ?? "queued",
        }));
        if (jobId) watchSyncProgress(row.connection.id, jobId, setSyncProgress, eventSourcesRef, refresh);
      }
      refresh();
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Integration action failed.");
    } finally {
      setActionBusy(null);
    }
  };

  return (
    <>
      <SectionHeader title="Workspace integrations" description="Connect shared systems used by the workspace." />
      {summary ? (
        <div className="mb-4 grid gap-3 sm:grid-cols-3">
          <Metric
            label="Connected"
            value={String(summary.metrics.connected)}
            detail={`${summary.metrics.readyProviders} providers configured`}
          />
          <Metric
            label="Syncing"
            value={String(summary.metrics.syncing)}
            detail="Live sync jobs visible from integration-corev2"
          />
          <Metric
            label="Attention"
            value={String(summary.metrics.failed)}
            detail="Sources needing reconnect or review"
          />
        </div>
      ) : null}
      <div className="divide-y divide-[#E8E8EA] overflow-hidden rounded-[18px] border border-[#E1E2E4] bg-white/42 dark:divide-white/10 dark:border-white/10 dark:bg-white/5">
        {rows.map((integration) => {
          const rowSyncProgress =
            "connection" in integration && integration.connection
              ? syncProgress[integration.connection.id]
              : undefined;
          return (
            <div key={integration.name} className="flex items-center justify-between gap-4 px-5 py-4">
              <div className="min-w-0">
                <p className="text-[13px] font-medium text-[#111111] dark:text-white">{integration.name}</p>
                <p className="mt-1 truncate text-[12px] text-[#737780] dark:text-[#A9ADB6]">
                  {rowSyncProgress ? `${integration.detail} · ${rowSyncProgress}` : integration.detail}
                </p>
              </div>
              <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
                <span
                  className={cn(
                    "rounded-full border px-2.5 py-1 text-[11px] font-medium",
                    integration.status === "Connected" ? "border-[#1E7A45]/20 text-[#1E7A45]" : "",
                    integration.status === "Missing config" ? "border-[#B45309]/20 text-[#B45309]" : "",
                    integration.status === "Ready" ? "border-[#4256D6]/20 text-[#4256D6]" : "",
                  )}
                >
                  {integration.status}
                </span>
                {"provider" in integration ? (
                  <IntegrationRowActions
                    busyKey={actionBusy}
                    row={integration}
                    onAction={runAction}
                  />
                ) : null}
              </div>
            </div>
          );
        })}
      </div>
      <p className="mt-3 text-[12px] leading-5 text-[#737780] dark:text-[#A9ADB6]">
        Source contents and graph edits are managed later in Knowledge, where changes can be reviewed and audited.
        {loadFailed ? " Integration state could not be refreshed from the local services." : ""}
      </p>
      {notice ? (
        <p className="mt-2 text-[12px] leading-5 text-[#5E635B] dark:text-[#C8CED8]" role="status">
          {notice}
        </p>
      ) : null}
      <FeaturePanel
        title="Webhook delivery"
        description="Delivery health for shared workspace automations."
        actionLabel="View logs"
        className="mt-8"
      >
        <div className="divide-y divide-[#E8E8EA] dark:divide-white/10">
          {webhookRows.map((row) => (
            <DataRow
              key={row.endpoint}
              primary={row.endpoint}
              secondary={row.lastRun}
              meta={row.status}
            />
          ))}
        </div>
      </FeaturePanel>
    </>
  );
}

function buildIntegrationRows(summary: IntegrationSettingsSummary | null): Array<IntegrationSettingsRow | (typeof fallbackIntegrationRows)[number]> {
  if (!summary) return fallbackIntegrationRows;

  const connectionsByProvider = new Map(summary.connections.map((connection) => [connection.providerKey, connection]));
  return summary.providers.map((provider) => {
    const connection = connectionsByProvider.get(provider.key);
    if (connection) {
      return {
        name: provider.label,
        detail: [
          connection.displayName,
          connection.capabilities.length > 0 ? `${connection.capabilities.length} capabilities` : null,
          connection.scopeCount > 0 ? `${connection.scopeCount} scopes` : null,
          connection.latestSyncJob?.status ? `sync ${connection.latestSyncJob.status}` : null,
        ].filter(Boolean).join(" · "),
        status: "Connected",
        action: "connected",
        connection,
        provider,
      };
    }

    if (!provider.configured) {
      return {
        name: provider.label,
        detail: provider.missingConfig.length > 0
          ? `Missing ${provider.missingConfig.slice(0, 2).join(", ")}`
          : "Provider credentials are not configured",
        status: "Missing config",
        action: "missing",
        provider,
      };
    }

    if (!provider.directOAuthReady) {
      return {
        name: provider.label,
        detail: `${provider.category} adapter is configured for admin setup`,
        status: "Admin setup",
        action: "admin",
        provider,
      };
    }

    return {
      name: provider.label,
      detail: `${provider.category} source · ${provider.capabilities.length} capabilities`,
      status: "Ready",
      action: "connect",
      provider,
    };
  });
}

function IntegrationRowActions({
  busyKey,
  onAction,
  row,
}: {
  busyKey: string | null;
  onAction: (row: IntegrationSettingsRow, action: "connect" | "disconnect" | "reconnect" | "sync") => void;
  row: IntegrationSettingsRow;
}) {
  const busy = busyKey?.startsWith(`${row.provider.key}:`) ?? false;
  if (row.action === "missing" || row.action === "admin") {
    return null;
  }
  if (row.action === "connect") {
    return (
      <VerevonButton size="sm" radius="sm" disabled={busy} onClick={() => onAction(row, "connect")} className="px-3 text-[12px]">
        {busy ? "Opening" : "Connect"}
      </VerevonButton>
    );
  }
  return (
    <>
      <VerevonButton size="sm" radius="sm" disabled={busy} onClick={() => onAction(row, "sync")} className="px-3 text-[12px]">
        Sync
      </VerevonButton>
      <VerevonButton size="sm" radius="sm" disabled={busy} onClick={() => onAction(row, "reconnect")} className="px-3 text-[12px]">
        Reconnect
      </VerevonButton>
      <VerevonButton size="sm" radius="sm" disabled={busy} onClick={() => onAction(row, "disconnect")} className="px-3 text-[12px] text-[#B42318]">
        Disconnect
      </VerevonButton>
    </>
  );
}

function runSettingsOAuth(session: ConnectSessionResult): Promise<void> {
  if (session.authMode !== "direct-oauth" || !session.connectUrl || !session.sessionToken) {
    return Promise.reject(new Error("Integration service returned an incomplete connect session."));
  }
  const authWindow = openSettingsOAuthWindow();
  if (!authWindow) {
    return Promise.reject(new Error("The provider sign-in window was blocked by the browser."));
  }

  return new Promise((resolve, reject) => {
    let settled = false;
    const settle = (callback: () => void) => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timeout);
      window.clearInterval(closePoll);
      window.removeEventListener("message", onMessage);
      callback();
    };
    const onMessage = (event: MessageEvent) => {
      const payload = event.data;
      if (!payload || typeof payload !== "object") return;
      const record = payload as Record<string, unknown>;
      if (record.type !== "verevon.integration.connected") return;
      if (record.sessionToken !== session.sessionToken) return;
      if (record.status === "success") {
        settle(resolve);
        return;
      }
      settle(() => reject(new Error(typeof record.message === "string" ? record.message : "Provider authorization failed.")));
    };
    window.addEventListener("message", onMessage);
    const timeout = window.setTimeout(() => {
      settle(() => reject(new Error("Provider authorization timed out.")));
    }, 120_000);
    const closePoll = window.setInterval(() => {
      if (!authWindow.closed) return;
      settle(() => reject(new Error("Provider authorization was closed before it finished.")));
    }, 500);
    try {
      authWindow.location.href = session.connectUrl;
    } catch {
      settle(() => reject(new Error("Provider authorization was closed before it finished.")));
    }
  });
}

function openSettingsOAuthWindow(): Window | null {
  const width = Math.min(540, window.screen.width);
  const height = Math.min(720, window.screen.height);
  const left = Math.max(window.screen.width / 2 - width / 2, 0);
  const top = Math.max(window.screen.height / 2 - height / 2, 0);
  return window.open(
    "",
    "_blank",
    [
      `left=${left}`,
      `top=${top}`,
      `width=${width}`,
      `height=${height}`,
      "scrollbars=yes",
      "resizable=yes",
      "status=no",
      "toolbar=no",
      "location=no",
      "menubar=no",
    ].join(","),
  );
}

function watchSyncProgress(
  connectionId: string,
  jobId: string,
  setSyncProgress: Dispatch<SetStateAction<Record<string, string>>>,
  eventSourcesRef: MutableRefObject<EventSource[]>,
  onTerminal?: () => void,
) {
  const source = new EventSource(`/api/v1/integrations/sync-jobs/${encodeURIComponent(jobId)}/events`);
  eventSourcesRef.current = [...eventSourcesRef.current, source];
  const close = () => {
    source.close();
    eventSourcesRef.current = eventSourcesRef.current.filter((item) => item !== source);
  };
  const update = (event: MessageEvent) => {
    const status = syncStatusFromEvent(event);
    if (!status) return;
    setSyncProgress((prev) => ({ ...prev, [connectionId]: status }));
    if (["completed", "failed", "cancelled"].includes(status)) {
      close();
      onTerminal?.();
    }
  };
  source.addEventListener("sync.queued", update);
  source.addEventListener("sync.running", update);
  source.addEventListener("sync.waiting_provider", update);
  source.addEventListener("sync.handoff_data_plane", update);
  source.addEventListener("sync.completed", update);
  source.addEventListener("sync.failed", update);
  source.addEventListener("sync.cancelled", update);
  source.addEventListener("sync.snapshot", update);
  source.onerror = () => {
    close();
  };
}

function syncStatusFromEvent(event: MessageEvent): string | null {
  try {
    const payload = JSON.parse(String(event.data)) as {
      type?: string;
      syncJob?: { status?: unknown };
    };
    if (payload.syncJob && typeof payload.syncJob.status === "string") return payload.syncJob.status;
    if (typeof payload.type === "string") return payload.type.replace(/^sync\./, "");
  } catch {
    return null;
  }
  return null;
}

function FeaturePanel({
  actionLabel,
  children,
  className,
  description,
  title,
}: {
  actionLabel: string;
  children: ReactNode;
  className?: string;
  description: string;
  title: string;
}) {
  return (
    <div className={cn("rounded-[18px] border border-[#E1E2E4] bg-white/42 p-5 dark:border-white/10 dark:bg-white/5", className)}>
      <div className="mb-4 flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h3 className="text-[15px] font-semibold text-[#111111] dark:text-white">{title}</h3>
          <p className="mt-1 max-w-[520px] text-[12px] leading-5 text-[#737780] dark:text-[#A9ADB6]">{description}</p>
        </div>
        <VerevonButton size="xs" radius="sm" className="shrink-0 px-3 text-[12px]">
          {actionLabel}
        </VerevonButton>
      </div>
      {children}
    </div>
  );
}

function DataRow({
  meta,
  primary,
  secondary,
}: {
  meta: string;
  primary: string;
  secondary: string;
}) {
  return (
    <div className="grid gap-2 py-3 first:pt-0 last:pb-0 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
      <div className="min-w-0">
        <p className="truncate text-[13px] font-medium text-[#111111] dark:text-white">{primary}</p>
        <p className="mt-1 truncate text-[12px] text-[#737780] dark:text-[#A9ADB6]">{secondary}</p>
      </div>
      <span className="w-fit rounded-[9px] bg-[#F0F1F5] px-2.5 py-1 text-[12px] font-medium text-[#4D5159] dark:bg-white/10 dark:text-[#C9CDD5]">
        {meta}
      </span>
    </div>
  );
}

function SectionHeader({ title, description }: { title: string; description: string }) {
  return (
    <div className="mb-7">
      <h2 className="text-[27px] font-semibold leading-tight tracking-normal text-[#111111] dark:text-white">{title}</h2>
      <p className="mt-2 max-w-[660px] text-[13px] leading-5 text-[#6A6E77] dark:text-[#A9ADB6]">{description}</p>
    </div>
  );
}

function Metric({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="rounded-[18px] border border-[#E1E2E4] bg-white/42 px-5 py-4 dark:border-white/10 dark:bg-white/5">
      <p className="text-[11px] font-medium uppercase tracking-[0.16em] text-[#9A9DA5]">{label}</p>
      <p className="mt-2 text-[22px] font-semibold text-[#111111] dark:text-white">{value}</p>
      <p className="mt-1 text-[12px] text-[#737780] dark:text-[#A9ADB6]">{detail}</p>
    </div>
  );
}

function SettingsField({
  id,
  label,
  defaultValue,
  placeholder,
  type = "text",
}: {
  id: string;
  label: string;
  defaultValue?: string;
  placeholder?: string;
  type?: "email" | "text";
}) {
  return (
    <label htmlFor={id} className="block">
      <span className="verevon-settings-label">{label}</span>
      <VerevonInput
        id={id}
        type={type}
        defaultValue={defaultValue}
        placeholder={placeholder}
        variant="settings"
        className="-mt-2"
      />
    </label>
  );
}

function SettingsSelect({
  id,
  label,
  defaultValue,
  options,
}: {
  id: string;
  label: string;
  defaultValue: string;
  options: Array<{ value: string; label: string }>;
}) {
  return (
    <label htmlFor={id} className="block">
      <span className="verevon-settings-label">{label}</span>
      <div className="relative -mt-2">
        <VerevonSelect
          id={id}
          defaultValue={defaultValue}
          variant="settings"
          className="appearance-none"
        >
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </VerevonSelect>
        <ChevronDown
          aria-hidden="true"
          className="pointer-events-none absolute right-5 top-1/2 size-4 -translate-y-1/2 text-[#6F737C]"
          strokeWidth={1.7}
        />
      </div>
    </label>
  );
}

function ToggleRow({
  title,
  description,
  enabled,
}: {
  title: string;
  description: string;
  enabled: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-5 py-5">
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-[#111111] dark:text-white">{title}</p>
        <p className="mt-1 text-[12px] leading-5 text-[#737780] dark:text-[#A9ADB6]">{description}</p>
      </div>
      <VerevonSwitch checked={enabled} label={title} />
    </div>
  );
}
