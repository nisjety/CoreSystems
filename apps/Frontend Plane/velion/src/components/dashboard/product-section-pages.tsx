'use client';

import Link from 'next/link';
import {
  Activity,
  AtSign,
  Bot,
  BrainCircuit,
  CheckCheck,
  ChevronDown,
  ChevronRight,
  CircleDashed,
  Clock3,
  Inbox,
  LayoutGrid,
  LifeBuoy,
  Mail,
  MessageSquare,
  MoreHorizontal,
  PenLine,
  Search,
  ShieldAlert,
  SlidersHorizontal,
  Sparkles,
  UserRound,
  Users,
} from 'lucide-react';

import {
  ActivityFeed,
  ContextStack,
  InlineStatPills,
  LinkGrid,
  ListTable,
  MetricStrip,
  ProductPageShell,
  SectionNavList,
  SplitWorkspace,
  SurfaceCard,
  type ProductListItem,
} from './product-page-templates';

export function OverviewWorkspacePage({ currentViewId }: { currentViewId?: string }) {
  const viewLabel = currentViewId === 'activity'
    ? 'Activity'
    : currentViewId === 'recents'
      ? 'Recents'
      : currentViewId === 'leads'
        ? 'Leads'
        : 'Dashboard';

  return (
    <ProductPageShell
      eyebrow="Overview"
      title={viewLabel === 'Dashboard' ? 'Operational dashboard' : `Overview · ${viewLabel}`}
      description="A calm command layer for queue health, urgent issues, AI performance, and the next actions your team should take."
      actions={[
        { label: 'Open inbox', href: '/inbox' },
        { label: 'Open helpdesk', href: '/helpdesk' },
        { label: 'Open reports', href: '/reports' },
      ]}
      headerAside={
        <InlineStatPills
          items={[
            { label: 'Coverage', value: '89%' },
            { label: 'AI assist', value: '63%' },
            { label: 'Open risks', value: '7' },
          ]}
        />
      }
    >
      <MetricStrip
        metrics={[
          { label: "Today's volume", value: '482', trend: '+12%', tone: 'accent' },
          { label: 'Unassigned', value: '28', trend: 'Needs routing', tone: 'warning' },
          { label: 'SLA risk', value: '7', trend: '2 critical', tone: 'warning' },
          { label: 'AI resolution rate', value: '63%', trend: '+4 pts', tone: 'success' },
        ]}
      />

      <div className="grid gap-4 xl:grid-cols-[1.15fr_0.85fr]">
        <SurfaceCard
          eyebrow="Summary"
          title="What needs attention now"
          description="The dashboard should behave like a support operator landing page rather than a generic workspace home."
        >
          <ActivityFeed
            items={[
              {
                id: 'recent-issue-1',
                title: 'Top channel: Messenger',
                description: 'Messenger drove 39% of today’s inbound volume, with most escalation risk coming from delivery and billing questions.',
                meta: 'Live',
              },
              {
                id: 'recent-issue-2',
                title: 'Recent issue cluster',
                description: 'A new spike in failed order updates is creating repeat tickets across inbox and helpdesk.',
                meta: 'Urgent',
              },
              {
                id: 'recent-issue-3',
                title: 'Team focus',
                description: 'Nordic support has the highest closure velocity today. Spanish support needs routing help before the next SLA checkpoint.',
                meta: 'Insight',
              },
            ]}
          />
        </SurfaceCard>

        <SurfaceCard
          eyebrow="Jump points"
          title="Move directly into the work"
          description="Overview should feed the operating surfaces instead of duplicating them."
        >
          <LinkGrid
            items={[
              { title: 'Inbox triage', description: 'Review unassigned, mentions, and queue backlog.', href: '/inbox' },
              { title: 'Helpdesk tickets', description: 'Inspect open tasks, escalations, and SLA breaches.', href: '/helpdesk' },
              { title: 'Contacts', description: 'Open customer records with recent conversation context.', href: '/people' },
              { title: 'Reports', description: 'Review channel, team, and AI performance.', href: '/reports' },
            ]}
          />
        </SurfaceCard>
      </div>
    </ProductPageShell>
  );
}

export function TasksWorkspacePage() {
  const items: ProductListItem[] = [
    { id: 'task-1', title: 'Refund backlog review', subtitle: 'Billing escalation queue', meta: 'Owner · Nora', tone: 'warning' },
    { id: 'task-2', title: 'Macro audit for shipping delays', subtitle: 'Helpdesk macro library', meta: 'Due today', tone: 'accent' },
    { id: 'task-3', title: 'Reconnect Klarna ticket workflow', subtitle: 'Integration follow-up', meta: 'Blocked', tone: 'warning' },
    { id: 'task-4', title: 'Review AI handoff prompts', subtitle: 'Agent studio', meta: 'Ready', tone: 'success' },
  ];

  return (
    <ProductPageShell
      eyebrow="Tasks"
      title="Tasks and tickets"
      description="A Zendesk-leaning operator board that mixes personal tasks with ticket-driven follow-ups so execution stays tied to support work."
      actions={[
        { label: 'Open helpdesk', href: '/helpdesk' },
        { label: 'Open inbox', href: '/inbox' },
      ]}
    >
      <div className="grid gap-4 xl:grid-cols-[1.15fr_0.85fr]">
        <SurfaceCard eyebrow="Execution" title="Assigned work" description="Track follow-ups from inbox, helpdesk, and AI handoffs in one place.">
          <ListTable items={items} columns={[{ key: 'subtitle', label: 'Source' }, { key: 'meta', label: 'State' }]} />
        </SurfaceCard>

        <SurfaceCard eyebrow="Ticketing" title="SLA watch" description="Use the task view to catch tickets that need ownership before they become breaches.">
          <ActivityFeed
            items={[
              { id: 'sla-1', title: '7 tickets nearing SLA breach', description: 'Most are shipping and returns conversations that lost ownership during handoff.', meta: '45m' },
              { id: 'sla-2', title: '4 tasks waiting on engineering', description: 'Escalations are ready but missing internal updates.', meta: 'Blocked' },
              { id: 'sla-3', title: '2 VIP contacts awaiting reply', description: 'Priority customers have open follow-up tasks linked to billing cases.', meta: 'Priority' },
            ]}
          />
        </SurfaceCard>
      </div>
    </ProductPageShell>
  );
}

export function ContactsWorkspacePage() {
  return (
    <ProductPageShell
      eyebrow="Contacts"
      title="Customer and account records"
      description="Profiles, activity history, ownership, and linked conversations live here so operators can move from inbox triage into full customer context."
      actions={[
        { label: 'Open inbox', href: '/inbox' },
        { label: 'Open reports', href: '/reports' },
      ]}
      headerAside={
        <InlineStatPills
          items={[
            { label: 'Open contacts', value: '1,482' },
            { label: 'VIP accounts', value: '46' },
            { label: 'Needs owner', value: '12' },
          ]}
        />
      }
    >
      <div className="grid gap-4 xl:grid-cols-[0.9fr_1.1fr]">
        <SurfaceCard eyebrow="Directory" title="Recently active contacts" description="The primary record list should stay calm, readable, and useful for support and account work.">
          <ListTable
            items={[
              { id: 'contact-1', title: 'Amelia Hughes', subtitle: 'Nordic Interiors', meta: '3 open conversations', tone: 'warning' },
              { id: 'contact-2', title: 'Diego Ramirez', subtitle: 'Atelier North', meta: 'Assigned · Hanna', tone: 'accent' },
              { id: 'contact-3', title: 'Lina Madsen', subtitle: 'Independent buyer', meta: 'VIP', tone: 'success' },
              { id: 'contact-4', title: 'Marcus Green', subtitle: 'Homeform', meta: 'Needs follow-up', tone: 'warning' },
            ]}
            columns={[{ key: 'subtitle', label: 'Account' }, { key: 'meta', label: 'Context' }]}
          />
        </SurfaceCard>

        <SurfaceCard eyebrow="Record" title="Selected contact" description="Contacts should combine CRM structure with support context instead of acting like a generic user table.">
          <div className="grid gap-4 lg:grid-cols-[0.8fr_1.2fr]">
            <div className="rounded-[22px] border border-[#EEE8DC] bg-white/92 p-5">
              <div className="flex items-center gap-3">
                <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-[#EEF2FF] text-[#294FCB]">
                  <Users className="h-5 w-5" />
                </div>
                <div>
                  <div className="text-[18px] font-semibold tracking-[-0.03em] text-[#1F2229]">Amelia Hughes</div>
                  <div className="text-[13px] text-[#666A73]">Senior buyer · Nordic Interiors</div>
                </div>
              </div>

              <div className="mt-5 space-y-2 text-[13px]">
                <div className="flex justify-between gap-3"><span className="text-[#8D877D]">Owner</span><span className="font-medium text-[#2E3445]">Hanna Pettersen</span></div>
                <div className="flex justify-between gap-3"><span className="text-[#8D877D]">Segment</span><span className="font-medium text-[#2E3445]">VIP retail</span></div>
                <div className="flex justify-between gap-3"><span className="text-[#8D877D]">Last purchase</span><span className="font-medium text-[#2E3445]">3 days ago</span></div>
                <div className="flex justify-between gap-3"><span className="text-[#8D877D]">Open conversations</span><span className="font-medium text-[#2E3445]">3</span></div>
              </div>
            </div>

            <ActivityFeed
              items={[
                { id: 'contact-history-1', title: 'Returns inquiry moved from inbox to helpdesk', description: 'Conversation was converted into a ticket because warehouse approval is needed.', meta: 'Today' },
                { id: 'contact-history-2', title: 'Assigned to Hanna Pettersen', description: 'Ownership changed after the account crossed VIP volume thresholds.', meta: 'Yesterday' },
                { id: 'contact-history-3', title: 'AI generated draft approved', description: 'A refund clarification draft was accepted with minor edits.', meta: '2d ago' },
              ]}
            />
          </div>
        </SurfaceCard>
      </div>
    </ProductPageShell>
  );
}

export { InboxWorkspacePage } from '@/components/inbox/InboxWorkspacePage'

export function HelpdeskWorkspacePage() {
  return (
    <div className="flex h-full min-h-0 overflow-hidden pr-3 md:pr-4 bg-white">
      <div className="grid h-full w-full min-w-0 xl:grid-cols-[280px_minmax(0,1fr)] xl:gap-0">
        <section className="h-full overflow-y-auto border-r border-[#E9EBF2] bg-white flex flex-col pb-4">
          <div className="px-5 mt-8 mb-4 text-[12px] font-semibold uppercase tracking-wider text-[#9CA3AF]">
            Macros
          </div>
          <nav className="space-y-0.5 px-3">
            {['Refund Request', 'Shipping Delay', 'Faulty Product', 'Account Access'].map((macro) => (
              <button
                key={macro}
                type="button"
                className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-[13px] text-[#4B5563] transition-colors hover:bg-gray-100"
              >
                <Sparkles className="size-3.5 text-gray-400" />
                <span className="truncate">{macro}</span>
              </button>
            ))}
          </nav>
        </section>

        {/* Middle: Helpdesk conversation and list */}
        <main className="my-3 overflow-hidden rounded-[16px] border border-[#E9EBF2] bg-white shadow-[0_8px_24px_rgba(22,20,17,0.04)] md:my-4 flex flex-col">
          {/* Header */}
          <div className="border-b border-[#E9EBF2] px-5 py-4">
            <div className="flex items-center justify-between gap-4">
              <div className="flex items-center gap-3">
                <span className="rounded bg-orange-100 px-2 py-0.5 text-[12px] font-semibold text-orange-700">#41203</span>
                <span className="text-[16px] font-semibold text-[#1F2229]">Replacement order missing carrier scan</span>
              </div>
              <div className="flex gap-2">
                <button className="rounded-[8px] border border-gray-200 px-3 py-1.5 text-[12px] font-medium text-gray-700 hover:bg-gray-50">Assign to me</button>
                <button className="rounded-[8px] border border-gray-200 px-3 py-1.5 text-[12px] font-medium text-gray-700 hover:bg-gray-50">Merge</button>
                <button className="rounded-[8px] bg-red-500 px-3 py-1.5 text-[12px] font-medium text-white hover:bg-red-600">Close as solved</button>
              </div>
            </div>
            <div className="mt-2 text-[13px] text-[#6D7280]">
              Requested by <span className="font-medium text-gray-900">Amelia Hughes</span> · 14 hours ago via Email
            </div>
          </div>

          {/* Conversation Feed */}
          <div className="flex-1 overflow-y-auto px-5 py-6 bg-[#F9FAFB]">
            <div className="space-y-6 max-w-3xl mx-auto">
              {/* Customer message */}
              <div className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm">
                <div className="flex items-center justify-between border-b border-gray-100 pb-3">
                  <div className="flex items-center gap-3">
                    <div className="flex size-8 items-center justify-center rounded-full bg-[#9CC4FF] text-[13px] font-medium text-white">AH</div>
                    <div>
                      <div className="text-[13px] font-semibold text-gray-900">Amelia Hughes</div>
                      <div className="text-[11px] text-gray-500">amelia@nordicinteriors.co</div>
                    </div>
                  </div>
                  <span className="text-[12px] text-gray-500">Yesterday at 14:23</span>
                </div>
                <div className="pt-3 text-[14px] leading-relaxed text-gray-800">
                  <p>Hi,</p>
                  <p className="mt-2">My replacement for order #10924 still hasn't received a carrier scan. It says shipped, but PostNord has no record of it. Can you look into this?</p>
                </div>
              </div>

              {/* Internal Note */}
              <div className="rounded-xl border border-yellow-200 bg-yellow-50 p-4 shadow-sm">
                <div className="flex items-center justify-between border-b border-yellow-200/50 pb-3">
                  <div className="flex items-center gap-3">
                    <div className="flex size-8 items-center justify-center rounded-full bg-yellow-200 text-[13px] font-medium text-yellow-800"><PenLine className="size-4" /></div>
                    <div>
                      <div className="text-[13px] font-semibold text-gray-900">System Note</div>
                      <div className="text-[11px] text-gray-500">AI Insight</div>
                    </div>
                  </div>
                  <span className="text-[12px] text-gray-500">Yesterday at 14:24</span>
                </div>
                <div className="pt-3 text-[14px] leading-relaxed text-yellow-900">
                  <p>Order #10924-R is stuck in "Manifested" state. High likelihood the label was printed but the parcel missed the pickup truck. SLA risk threshold approaching.</p>
                </div>
              </div>
            </div>
          </div>

          {/* Reply composer */}
          <div className="border-t border-[#E9EBF2] bg-white p-4">
            <div className="rounded-xl border border-gray-200 outline-none focus-within:border-blue-500 focus-within:ring-1 focus-within:ring-blue-500 overflow-hidden text-[14px]">
              <div className="flex items-center gap-4 bg-gray-50 px-4 py-2 border-b border-gray-200 text-[13px] font-medium text-gray-600">
                <button className="text-blue-600 hover:text-blue-800">Public Reply</button>
                <button className="hover:text-gray-900">Internal Note</button>
              </div>
              <textarea 
                className="w-full resize-none bg-white p-4 outline-none placeholder:text-gray-400" 
                rows={4} 
                placeholder="Type your reply here..."
              />
              <div className="flex items-center justify-between bg-white px-4 py-2 border-t border-gray-100">
                <button className="flex items-center gap-2 rounded-md px-2 py-1 text-[12px] text-gray-500 hover:bg-gray-100"><Sparkles className="size-3.5" /> Apply macro...</button>
                <div className="flex items-center gap-2">
                  <button className="rounded-lg bg-blue-600 px-4 py-1.5 text-[13px] font-medium text-white hover:bg-blue-700 shadow-sm">Submit as Open</button>
                  <button className="rounded-lg bg-gray-100 p-1.5 hover:bg-gray-200"><ChevronDown className="size-4" /></button>
                </div>
              </div>
            </div>
          </div>
        </main>

        {/* Right: Gorgias-style Order / Customer Context */}
        <aside className="my-3 overflow-hidden rounded-[16px] border border-[#E9EBF2] bg-white shadow-[0_8px_24px_rgba(22,20,17,0.04)] md:my-4 flex flex-col">
          <div className="border-b border-gray-200 bg-gray-50 px-5 py-3">
            <h3 className="text-[13px] font-semibold text-gray-800 uppercase tracking-wider">Customer Profile</h3>
          </div>
          
          <div className="flex-1 overflow-y-auto">
            {/* Customer Identity */}
            <div className="border-b border-gray-100 p-5">
              <div className="flex items-center gap-3">
                <div className="flex size-10 items-center justify-center rounded-full bg-[#9CC4FF] text-[15px] font-medium text-white">AH</div>
                <div>
                  <div className="text-[15px] font-semibold text-gray-900">Amelia Hughes</div>
                  <div className="text-[13px] text-blue-600">VIP B2B Client</div>
                </div>
              </div>
              <div className="mt-4 space-y-2 text-[13px]">
                <div className="flex items-center gap-2 text-gray-600"><Mail className="size-3.5" /> amelia@nordicinteriors.co</div>
                <div className="flex items-center gap-2 text-gray-600"><CheckCheck className="size-3.5" /> Ordered 14 times</div>
                <div className="flex items-center gap-2 text-gray-600"><Activity className="size-3.5" /> LTV: $12,450.00</div>
              </div>
            </div>

            <div className="border-b border-gray-200 bg-gray-50 px-5 py-3 flex justify-between items-center">
              <h3 className="text-[13px] font-semibold text-gray-800 uppercase tracking-wider">Recent Orders</h3>
              <button className="text-[12px] text-blue-600 hover:underline">View all (14)</button>
            </div>

            {/* Shopify-like Order Widget */}
            <div className="p-5 space-y-4">
              <div className="rounded-lg border border-gray-200 bg-white p-3 shadow-sm">
                <div className="flex items-center justify-between mb-2">
                  <a href="#" className="text-[14px] font-semibold text-blue-600 hover:underline">#10924-R</a>
                  <span className="rounded bg-yellow-100 px-2 py-0.5 text-[11px] font-medium text-yellow-800">Unfulfilled</span>
                </div>
                <div className="text-[12px] text-gray-500 mb-3">Placed 4 days ago · $0.00 (Replacement)</div>
                
                <div className="border-t border-gray-100 pt-3 flex items-center justify-between">
                  <div className="text-[13px] text-gray-700">1x Nordic Wool Throw</div>
                </div>
                <div className="mt-3 flex gap-2">
                  <button className="flex-1 rounded-md border border-gray-200 py-1 text-[12px] text-gray-700 hover:bg-gray-50">Cancel</button>
                  <button className="flex-1 rounded-md border border-gray-200 py-1 text-[12px] text-gray-700 hover:bg-gray-50">Duplicate</button>
                </div>
              </div>

              <div className="rounded-lg border border-gray-200 bg-white p-3 shadow-sm opacity-60">
                <div className="flex items-center justify-between mb-2">
                  <a href="#" className="text-[14px] font-semibold text-blue-600 hover:underline">#10924</a>
                  <span className="rounded bg-green-100 px-2 py-0.5 text-[11px] font-medium text-green-800">Fulfilled</span>
                </div>
                <div className="text-[12px] text-gray-500 mb-3">Placed 12 days ago · $145.00</div>
              </div>
            </div>
            
            <div className="border-b border-gray-200 bg-gray-50 px-5 py-3">
              <h3 className="text-[13px] font-semibold text-gray-800 uppercase tracking-wider">Conversations</h3>
            </div>
            <div className="p-4 space-y-2">
              <div className="text-[13px] text-gray-800 font-medium">Replacement order missing carrier scan</div>
              <div className="text-[12px] text-gray-500">Currently open</div>
              
              <div className="text-[13px] text-gray-800 font-medium mt-3 border-t border-gray-100 pt-3">Damaged throw received</div>
              <div className="text-[12px] text-gray-500">Closed 4 days ago</div>
            </div>

          </div>
        </aside>
      </div>
    </div>
  );
}

export function ReportsWorkspacePage() {
  return (
    <ProductPageShell
      eyebrow="Reports"
      title="Support and AI performance"
      description="A denser analytics surface for team throughput, channel mix, AI outcomes, and resolution quality."
      actions={[
        { label: 'Open inbox', href: '/inbox' },
        { label: 'Open helpdesk', href: '/helpdesk' },
      ]}
    >
      <MetricStrip
        metrics={[
          { label: 'Median first response', value: '6m', trend: '-1m', tone: 'success' },
          { label: 'Resolution time', value: '4h 18m', trend: '-9%', tone: 'success' },
          { label: 'AI deflection', value: '31%', trend: '+6 pts', tone: 'accent' },
          { label: 'CSAT', value: '4.7/5', trend: 'Stable', tone: 'default' },
        ]}
      />

      <div className="grid gap-4 xl:grid-cols-[1fr_1fr]">
        <SurfaceCard eyebrow="Breakdowns" title="Team and channel view" description="Zendesk-style structure with Verevon’s softer visual system.">
          <ListTable
            items={[
              { id: 'report-1', title: 'Nordic support', subtitle: '84% of tickets within SLA', meta: 'Best closure velocity', tone: 'success' },
              { id: 'report-2', title: 'Messenger', subtitle: 'Highest inbound share today', meta: '39% of volume', tone: 'accent' },
              { id: 'report-3', title: 'Email', subtitle: 'Longest median handling time', meta: 'Needs review', tone: 'warning' },
              { id: 'report-4', title: 'AI handoffs', subtitle: 'Successful escalations', meta: '63% healthy', tone: 'accent' },
            ]}
            columns={[{ key: 'subtitle', label: 'Metric' }, { key: 'meta', label: 'Observation' }]}
          />
        </SurfaceCard>

        <SurfaceCard eyebrow="Insights" title="Recommended next actions" description="Reports should translate performance into operational moves, not just show charts.">
          <ActivityFeed
            items={[
              { id: 'insight-1', title: 'Expand shipping macros into inbox triage', description: 'Delay-related tickets are now the top repeat workflow across all channels.', meta: 'Action' },
              { id: 'insight-2', title: 'Increase AI review on email threads', description: 'Email has the slowest handling time and the largest assist opportunity.', meta: 'AI' },
              { id: 'insight-3', title: 'Rebalance team ownership before afternoon peak', description: 'One team carries most of the unassigned backlog.', meta: 'Staffing' },
            ]}
          />
        </SurfaceCard>
      </div>
    </ProductPageShell>
  );
}

export function AutomationsWorkspacePage() {
  return (
    <ProductPageShell
      eyebrow="Automations"
      title="Rules, triggers, and routing logic"
      description="Visible product language should say Automations, while route compatibility stays on /outbound for now."
      actions={[
        { label: 'Open helpdesk', href: '/helpdesk' },
        { label: 'Open agents', href: '/agents' },
      ]}
    >
      <div className="grid gap-4 xl:grid-cols-[0.85fr_1.15fr]">
        <SurfaceCard eyebrow="Rules" title="Automation library" description="Use a list + detail pattern instead of a decorative workflow canvas in this first pass.">
          <SectionNavList
            title="Automation groups"
            items={[
              { label: 'Routing rules', value: '12', active: true, hint: 'Assignee and queue logic' },
              { label: 'Macros', value: '18', hint: 'Operator shortcuts' },
              { label: 'Escalations', value: '6', hint: 'SLA and blocker rules' },
              { label: 'AI handoffs', value: '9', hint: 'Agent to human transitions' },
            ]}
          />
        </SurfaceCard>

        <SurfaceCard eyebrow="Selected rule" title="High-value routing rule" description="Support automations should look operational and explainable, not mysterious.">
          <div className="rounded-[24px] border border-[#EEE8DC] bg-white/92 p-5">
            <div className="flex items-center gap-3">
              <div className="flex h-11 w-11 items-center justify-center rounded-2xl bg-[#EEF2FF] text-[#294FCB]">
                <Sparkles className="h-5 w-5" />
              </div>
              <div>
                <div className="text-[17px] font-semibold text-[#1F2229]">Delay + VIP routing</div>
                <div className="text-[13px] text-[#666A73]">If shipping-delay intent + VIP account, assign to senior support and create helpdesk ticket.</div>
              </div>
            </div>

            <div className="mt-5 grid gap-3 md:grid-cols-3">
              {[
                { icon: Mail, title: 'Trigger', copy: 'Customer asks about delayed delivery' },
                { icon: Bot, title: 'Action', copy: 'Generate suggested reply and internal note' },
                { icon: LifeBuoy, title: 'Handoff', copy: 'Create ticket if warehouse confirmation is missing' },
              ].map((item) => {
                const Icon = item.icon;
                return (
                  <div key={item.title} className="rounded-[20px] border border-[#EEE8DC] bg-[#FCFBF8] p-4">
                    <div className="flex h-9 w-9 items-center justify-center rounded-2xl bg-[#F4F1EA] text-[#1F2229]">
                      <Icon className="h-4 w-4" />
                    </div>
                    <div className="mt-3 text-[14px] font-semibold text-[#1F2229]">{item.title}</div>
                    <div className="mt-1 text-[13px] leading-6 text-[#666A73]">{item.copy}</div>
                  </div>
                );
              })}
            </div>
          </div>
        </SurfaceCard>
      </div>
    </ProductPageShell>
  );
}

export function BuilderWorkspacePage({
  eyebrow,
  title,
  description,
  leftTitle,
  centerTitle,
  rightTitle,
}: {
  eyebrow: string;
  title: string;
  description: string;
  leftTitle: string;
  centerTitle: string;
  rightTitle: string;
}) {
  return (
    <ProductPageShell eyebrow={eyebrow} title={title} description={description}>
      <SplitWorkspace
        left={
          <SectionNavList
            title={leftTitle}
            items={[
              { label: 'Instructions', value: 'Live', active: true, hint: 'Core behavior' },
              { label: 'Knowledge sources', value: '12', hint: 'Connected sources' },
              { label: 'Actions', value: '6', hint: 'Allowed tools' },
              { label: 'Channels', value: '4', hint: 'Deployment targets' },
            ]}
          />
        }
        center={
          <div className="p-5">
            <div className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#9E978B]">
              {centerTitle}
            </div>
            <div className="mt-4 rounded-[22px] border border-[#EEE8DC] bg-white/92 p-5">
              <div className="text-[16px] font-semibold text-[#1F2229]">Production configuration</div>
              <p className="mt-2 text-[13px] leading-6 text-[#666A73]">
                This builder surface should separate live configuration from review and testing, borrowing Chatbase ergonomics but keeping the calmer Verevon shell.
              </p>
              <div className="mt-5 space-y-3">
                {[
                  'Instructions and tone controls',
                  'Knowledge source status and retrieval health',
                  'Actions, handoff rules, and channel permissions',
                ].map((line) => (
                  <div key={line} className="rounded-[18px] border border-[#EEE8DC] bg-[#FCFBF8] px-4 py-3 text-[13px] text-[#2E3445]">
                    {line}
                  </div>
                ))}
              </div>
            </div>
          </div>
        }
        right={
          <div className="p-5">
            <div className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#9E978B]">
              {rightTitle}
            </div>
            <div className="mt-4 space-y-3">
              <div className="rounded-[20px] border border-[#EEE8DC] bg-white/92 p-4">
                <div className="flex items-center gap-2 text-[13px] font-semibold text-[#1F2229]">
                  <BrainCircuit className="h-4 w-4" />
                  Playground
                </div>
                <p className="mt-2 text-[12px] leading-6 text-[#666A73]">
                  Test search, sample conversations, and handoff behavior should live next to configuration, not be buried behind navigation.
                </p>
              </div>
              <div className="rounded-[20px] border border-[#EEE8DC] bg-white/92 p-4">
                <div className="flex items-center gap-2 text-[13px] font-semibold text-[#1F2229]">
                  <CircleDashed className="h-4 w-4" />
                  Health checks
                </div>
                <p className="mt-2 text-[12px] leading-6 text-[#666A73]">
                  Show sync state, source health, and validation results in a compact utility panel.
                </p>
              </div>
            </div>
          </div>
        }
      />
    </ProductPageShell>
  );
}

const SETTINGS_LINKS = [
  { title: 'Integrations', description: 'Connect product systems and sync them into the knowledge and support layers.', href: '/settings/integrations' },
  { title: 'Members', description: 'Manage workspace membership, access boundaries, and roles.', href: '/settings/members' },
  { title: 'Billing', description: 'Review plan state, quotas, and billing controls.', href: '/settings/billing' },
  { title: 'Security', description: 'Review account protection, password posture, and sign-in controls.', href: '/settings/security' },
  { title: 'Permissions', description: 'Define workspace guardrails and access patterns.', href: '/settings/permissions' },
  { title: 'Notifications', description: 'Control workspace notifications and admin alerts.', href: '/settings/notifications' },
];

export function SettingsOverviewWorkspacePage() {
  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-white">
      <div className="mx-auto w-full max-w-[580px] px-6 py-10">
        <h1 className="mb-10 text-[22px] font-semibold tracking-tight text-[#111111]">
          Workspace administration
        </h1>
        <div className="border-t border-[#F0F0F0]">
          {SETTINGS_LINKS.map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className="flex items-center justify-between border-b border-[#F0F0F0] py-4 transition-colors hover:bg-[#FAFAFA] -mx-2 px-2 rounded-[6px]"
            >
              <div>
                <p className="text-[13px] font-medium text-[#111111]">{item.title}</p>
                <p className="mt-0.5 text-[12px] leading-5 text-[#6B7280]">{item.description}</p>
              </div>
              <ChevronRight className="h-4 w-4 shrink-0 text-[#BBBBBB]" />
            </Link>
          ))}
        </div>
      </div>
    </div>
  );
}

export function PlaceholderAdminSurface({
  eyebrow,
  title,
  description,
  ctaHref,
  ctaLabel,
}: {
  eyebrow: string;
  title: string;
  description: string;
  ctaHref?: string;
  ctaLabel?: string;
}) {
  return (
    <ProductPageShell eyebrow={eyebrow} title={title} description={description}>
      <SurfaceCard eyebrow="Planned surface" title={title} description={description} action={ctaHref && ctaLabel ? { href: ctaHref, label: ctaLabel } : undefined}>
        <p className="text-[14px] leading-7 text-[#666A73]">
          This area now inherits the shared Verevon admin shell and is ready for the next connected implementation slice.
        </p>
      </SurfaceCard>
    </ProductPageShell>
  );
}
