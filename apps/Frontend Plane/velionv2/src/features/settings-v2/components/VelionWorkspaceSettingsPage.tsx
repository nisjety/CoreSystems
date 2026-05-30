import { Check, ChevronDown, MoreHorizontal } from "lucide-react";
import type { ReactNode } from "react";
import {
  VelionButton,
  VelionIconButton,
  VelionInput,
  VelionSelect,
  VelionSwitch,
} from "@/components/ui/velion-ui";
import { cn } from "@/lib/utils";

export const workspaceSettingsSectionIds = [
  "workspace",
  "members",
  "billing",
  "sso",
  "org-security",
  "integrations",
] as const;

export type WorkspaceSettingsSectionId = (typeof workspaceSettingsSectionIds)[number];

type SectionDetail = {
  id: WorkspaceSettingsSectionId;
  label: string;
  title: string;
  description: string;
  saveLabel: string;
};

export const workspaceSettingsSections: SectionDetail[] = [
  // Scope notes:
  // Must have: workspace name, URL, primary domain, data region, default locale, business hours, admin owner, audit contact.
  // Nice to have: brand avatar, customer-facing assets, formatting defaults, DNS guidance, region migration readiness checks.
  {
    id: "workspace",
    label: "Workspace",
    title: "Workspace settings",
    description: "Manage shared identity, domains, regional defaults, and operational ownership.",
    saveLabel: "Save workspace",
  },
  // Scope notes:
  // Must have: invite members, role table, seat status, pending invites, remove access, ownership transfer.
  // Nice to have: role templates, temporary access expiry, bulk invite/import, SSO team mapping.
  {
    id: "members",
    label: "Members & roles",
    title: "Members & roles",
    description: "Control who has access, what they can do, and how seats are used.",
    saveLabel: "Save members",
  },
  // Scope notes:
  // Must have: current plan, renewal date, upgrade path, seat usage, conversation usage, payment method, invoices, usage caps.
  // Nice to have: spend forecast, cost-center tags, anomaly alerts, invoice downloads, tax settings.
  {
    id: "billing",
    label: "Billing",
    title: "Billing",
    description: "Review plan, usage, payment method, invoices, and spending controls.",
    saveLabel: "Save billing",
  },
  // Scope notes:
  // Must have: provider selection, verified domain, enforcement mode, SCIM token, connection testing.
  // Nice to have: attribute mapping, JIT provisioning, break-glass owner account, IdP metadata exchange.
  {
    id: "sso",
    label: "SSO",
    title: "SSO",
    description: "Configure organization sign-in, identity providers, domain enforcement, and provisioning.",
    saveLabel: "Save SSO",
  },
  // Scope notes:
  // Must have: MFA requirements, verified-domain sign-in, session duration, device review, admin audit log.
  // Nice to have: IP allowlist, anomaly alerts, export controls, retention and legal hold policy.
  {
    id: "org-security",
    label: "Org security",
    title: "Org security",
    description: "Set organization-wide security requirements, session policy, and audit controls.",
    saveLabel: "Save security",
  },
  // Scope notes:
  // Must have: connected app status, owner, OAuth scope review, sync health, retry, disconnect, credential rotation.
  // Nice to have: field mapping, webhook logs, sandbox mode, per-integration data retention controls.
  {
    id: "integrations",
    label: "Integrations",
    title: "Integrations",
    description: "Connect shared support, CRM, communication, and automation systems.",
    saveLabel: "Save integrations",
  },
];

const sectionDetails = Object.fromEntries(workspaceSettingsSections.map((section) => [section.id, section])) as Record<
  WorkspaceSettingsSectionId,
  SectionDetail
>;

const memberRows = [
  { name: "Author Name", email: "author@velion.ai", role: "Owner", status: "Active" },
  { name: "Mina Larsen", email: "mina@velion.ai", role: "Admin", status: "Active" },
  { name: "Ola Hansen", email: "ola@velion.ai", role: "Agent", status: "Invited" },
];

const integrationRows = [
  { name: "Intercom", detail: "Customer conversations", status: "Connect" },
  { name: "Zendesk", detail: "Tickets and customer profile sync", status: "Connected" },
  { name: "Gorgias", detail: "Commerce helpdesk sync", status: "Connect" },
  { name: "Slack", detail: "Internal escalations", status: "Connected" },
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
  members: [
    { label: "Seats", value: "3 / 5", detail: "2 seats available before plan upgrade.", tone: "ok" },
    { label: "Pending invites", value: "1", detail: "Ola Hansen has not accepted yet.", tone: "warn" },
    { label: "Access review", value: "Due in 21d", detail: "Owner review is scheduled for next month.", tone: "neutral" },
  ],
  billing: [
    { label: "Current plan", value: "Free", detail: "Upgrade available when seats or usage grow.", tone: "neutral" },
    { label: "Monthly usage", value: "18k", detail: "72% of the conversation allowance used.", tone: "warn" },
    { label: "Payment method", value: "Missing", detail: "Required before upgrading to a paid plan.", tone: "warn" },
  ],
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

const invoiceRows = [
  { invoice: "May 2026 estimate", amount: "$0", status: "Draft" },
  { invoice: "Apr 2026", amount: "$0", status: "Paid" },
  { invoice: "Mar 2026", amount: "$0", status: "Paid" },
];

const ssoMappingRows = [
  { attribute: "email", source: "primaryEmail", destination: "User email" },
  { attribute: "department", source: "orgUnitPath", destination: "Team" },
  { attribute: "role", source: "customSchema.velionRole", destination: "Workspace role" },
];

const auditRows = [
  { event: "MFA requirement enabled", actor: "Author Name", time: "Today, 09:42" },
  { event: "Zendesk token rotated", actor: "Mina Larsen", time: "Yesterday, 16:10" },
  { event: "Billing email changed", actor: "Author Name", time: "May 20, 2026" },
];

const webhookRows = [
  { endpoint: "Zendesk ticket sync", status: "200 OK", lastRun: "8 min ago" },
  { endpoint: "Slack escalation", status: "200 OK", lastRun: "14 min ago" },
  { endpoint: "CRM customer upsert", status: "Paused", lastRun: "2 days ago" },
];

export function isWorkspaceSettingsSection(value: string): value is WorkspaceSettingsSectionId {
  return workspaceSettingsSectionIds.includes(value as WorkspaceSettingsSectionId);
}

export function getWorkspaceSettingsSection(value: WorkspaceSettingsSectionId) {
  return sectionDetails[value];
}

export function VelionWorkspaceSettingsPage({
  section = "workspace",
}: {
  section?: WorkspaceSettingsSectionId;
}) {
  const details = sectionDetails[section];

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

          <StatusGrid cards={sectionStatusCards[section]} />

          <section id={details.id} className="mt-10 scroll-mt-24">
            <WorkspaceSettingsSection section={section} />
          </section>

          <div className="relative z-30 mt-10 flex items-center justify-end gap-3 border-t border-[#E8E8EA] pt-6 dark:border-white/10">
            <VelionButton className="px-5">
              Cancel
            </VelionButton>
            <VelionButton variant="primary" className="px-5">
              <Check className="size-4" strokeWidth={1.8} />
              {details.saveLabel}
            </VelionButton>
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

function WorkspaceSettingsSection({ section }: { section: WorkspaceSettingsSectionId }) {
  switch (section) {
    case "members":
      return <MembersSection />;
    case "billing":
      return <BillingSection />;
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
        <SettingsField id="workspace-url" label="Workspace URL" defaultValue="aquatiq-as.velion.ai" />
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

function MembersSection() {
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
        {memberRows.map((member) => (
          <div key={member.email} className="grid gap-3 border-b border-[#E8E8EA] px-5 py-4 last:border-b-0 sm:grid-cols-[1fr_120px_96px_32px] sm:items-center dark:border-white/10">
            <div className="min-w-0">
              <p className="truncate text-[13px] font-medium text-[#111111] dark:text-white">{member.name}</p>
              <p className="mt-1 truncate text-[12px] text-[#737780] dark:text-[#A9ADB6]">{member.email}</p>
            </div>
            <span className="text-[13px] font-medium text-[#4D5159] dark:text-[#CACDD4]">{member.role}</span>
            <span className="text-[12px] text-[#737780] dark:text-[#A9ADB6]">{member.status}</span>
            <VelionIconButton aria-label={`More actions for ${member.name}`} className="text-[#777B84] hover:bg-white dark:hover:bg-white/10">
              <MoreHorizontal className="size-4" strokeWidth={1.7} />
            </VelionIconButton>
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

function BillingSection() {
  return (
    <>
      <SectionHeader title="Plan & usage" description="Review plan, usage, payment method, and invoices." />
      <div className="grid gap-4 sm:grid-cols-3">
        <Metric label="Plan" value="Free" detail="Upgrade available" />
        <Metric label="Seats" value="3 / 5" detail="2 seats open" />
        <Metric label="Messages" value="18k" detail="This month" />
      </div>
      <div className="mt-6 grid gap-6 sm:grid-cols-2">
        <SettingsField id="billing-email" label="Billing email" type="email" defaultValue="billing@aquatiq.no" />
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
          <div className="divide-y divide-[#E8E8EA] dark:divide-white/10">
            {invoiceRows.map((invoice) => (
              <DataRow
                key={invoice.invoice}
                primary={invoice.invoice}
                secondary={invoice.status}
                meta={invoice.amount}
              />
            ))}
          </div>
        </FeaturePanel>
        <FeaturePanel
          title="Spend controls"
          description="Plan limits, usage alerts, and overage behavior for paid plans."
          actionLabel="Configure"
        >
          <div className="space-y-3 text-[13px] text-[#565B65] dark:text-[#B5BAC4]">
            <p className="flex items-center justify-between gap-4"><span>Forecast</span><strong className="font-semibold text-[#111111] dark:text-white">$42 / mo</strong></p>
            <p className="flex items-center justify-between gap-4"><span>Alert threshold</span><strong className="font-semibold text-[#111111] dark:text-white">80%</strong></p>
            <p className="flex items-center justify-between gap-4"><span>Overage action</span><strong className="font-semibold text-[#111111] dark:text-white">Notify admins</strong></p>
          </div>
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
          description="SCIM and SAML attributes mapped into Velion workspace fields."
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
        <div className="divide-y divide-[#E8E8EA] dark:divide-white/10">
          {auditRows.map((row) => (
            <DataRow
              key={`${row.event}-${row.time}`}
              primary={row.event}
              secondary={row.actor}
              meta={row.time}
            />
          ))}
        </div>
      </FeaturePanel>
    </>
  );
}

function IntegrationsSection() {
  return (
    <>
      <SectionHeader title="Workspace integrations" description="Connect shared systems used by the workspace." />
      <div className="divide-y divide-[#E8E8EA] overflow-hidden rounded-[18px] border border-[#E1E2E4] bg-white/42 dark:divide-white/10 dark:border-white/10 dark:bg-white/5">
        {integrationRows.map((integration) => (
          <div key={integration.name} className="flex items-center justify-between gap-4 px-5 py-4">
            <div className="min-w-0">
              <p className="text-[13px] font-medium text-[#111111] dark:text-white">{integration.name}</p>
              <p className="mt-1 truncate text-[12px] text-[#737780] dark:text-[#A9ADB6]">{integration.detail}</p>
            </div>
            <VelionButton size="sm" radius="sm" className="shrink-0 px-3 text-[12px]">
              {integration.status}
            </VelionButton>
          </div>
        ))}
      </div>
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
        <VelionButton size="xs" radius="sm" className="shrink-0 px-3 text-[12px]">
          {actionLabel}
        </VelionButton>
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
      <span className="velion-settings-label">{label}</span>
      <VelionInput
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
      <span className="velion-settings-label">{label}</span>
      <div className="relative -mt-2">
        <VelionSelect
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
        </VelionSelect>
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
      <VelionSwitch checked={enabled} label={title} />
    </div>
  );
}
