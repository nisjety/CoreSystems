"use client";

import {
  BarChart3,
  BriefcaseBusiness,
  CalendarClock,
  CheckCircle2,
  Database,
  Globe2,
  MessageSquareText,
  Rocket,
  Search,
  Settings2,
  ShieldCheck,
  ShoppingBag,
  Sparkles,
  Split,
  TicketCheck,
  Zap,
} from "lucide-react";
import { VerevonButton } from "@/components/ui/verevon-ui";
import {
  AgentFeatureBoard,
  CounterpartPanel,
  FeatureRow,
  RoleConversationPreview,
  StatusRow,
} from "@/features/agents-v2/components/VerevonAgentsWorkspacePrimitives";
import type { AgentFeatureId } from "@/features/agents-v2/lib/agent-roles";
import type {
  AgentBlueprint,
  RoleOperatingModel,
} from "@/features/agents-v2/lib/verevon-agent-page-types";
import {
  ecommerceAssistantModules,
  ecommerceCartRecoveryCards,
  ecommerceFinderChips,
  ecommerceInsightCards,
  ecommerceProductQuestions,
  ecommerceProductRecommendations,
  ecommerceQuickReplies,
  ecommerceSidekickTasks,
  ecommerceStoreActionPlan,
  ecommerceSupportRequests,
  salesBreezeResearch,
  salesCrmHandoffItems,
  salesEngagementModes,
  salesInsightMetrics,
  salesIntentSignals,
  salesLeadTags,
  salesMeetingSlots,
  salesObjectionRows,
  salesQualificationScores,
  salesVisitorIntelligenceSignals,
  serviceActionChecks,
  serviceChannelRollout,
  serviceGuidanceControls,
  serviceInsightQaRows,
  serviceNextImprovements,
  serviceResolutionQueue,
  serviceUnansweredTopics,
  serviceVerifiedQaRows,
} from "@/features/agents-v2/lib/verevon-agent-surface-data";
import {
  controlFocusClass,
  roleEyebrowClass,
  roleInsetClass,
  rolePanelClass,
} from "@/features/agents-v2/lib/verevon-agent-page-styles";
import { cn } from "@/lib/utils";

// RoleCounterpartSurface swaps the generic configuration grid for a role-native product surface.
export function RoleCounterpartSurface({
  feature,
  operatingModel,
  role,
}: {
  feature: AgentFeatureId;
  operatingModel: RoleOperatingModel;
  role: AgentBlueprint;
}) {
  if (role.id === "service") {
    return <ServiceResolutionSurface feature={feature} operatingModel={operatingModel} role={role} />;
  }

  if (role.id === "sales") {
    return <SalesSdrSurface feature={feature} operatingModel={operatingModel} role={role} />;
  }

  if (role.id === "ecommerce") {
    return <EcommerceCommerceSurface feature={feature} operatingModel={operatingModel} role={role} />;
  }

  return <AgentFeatureBoard operatingModel={operatingModel} role={role} />;
}

function ServiceResolutionSurface({
  feature,
  operatingModel,
  role,
}: {
  feature: AgentFeatureId;
  operatingModel: RoleOperatingModel;
  role: AgentBlueprint;
}) {
  if (feature === "service-knowledge") {
    return (
      <section className="grid gap-3 lg:grid-cols-[minmax(0,1.2fr)_minmax(280px,0.8fr)]" aria-label="Service knowledge workspace">
        <CounterpartPanel
          role={role}
          icon={Database}
          eyebrow="Intercom Fin style"
          title="Answer coverage map"
          description="Organize trusted support content by what customers actually ask before the agent answers at scale."
        >
          <div className="grid gap-2 md:grid-cols-3">
            {operatingModel.knowledge.map((source) => (
              <FeatureRow key={source.title} feature={source} role={role} />
            ))}
          </div>
          <div className="mt-3 grid gap-1.5 sm:grid-cols-4" aria-label="Fin-style guidance controls">
            {serviceGuidanceControls.map((item) => (
              <span key={item} className={cn("rounded-full border px-2 py-1 text-center text-[10px] font-semibold", roleInsetClass(role), roleEyebrowClass(role))}>
                {item}
              </span>
            ))}
          </div>
          <div className={cn("mt-3 rounded-[8px] border p-3", roleInsetClass(role))}>
            <div className="flex items-center justify-between text-[11px] font-semibold">
              <span className={roleEyebrowClass(role)}>Coverage health</span>
              <span className="text-[#202126] dark:text-white">Source audit</span>
            </div>
            <p className="mt-2 text-[11px] leading-4 text-[#68707B] dark:text-[#AEB4C0]">
              Live coverage metrics appear after approved sources and conversation events are connected.
            </p>
          </div>
        </CounterpartPanel>
        <CounterpartPanel
          role={role}
          icon={Search}
          eyebrow="Gap queue"
          title="Unanswered topics"
          description="Turn low-confidence answers into source requests instead of hidden failures."
        >
          {serviceUnansweredTopics.map((item, index) => (
            <div key={item} className={cn("mt-2 flex items-center justify-between rounded-[8px] border px-3 py-2 first:mt-0", roleInsetClass(role))}>
              <span className="text-[12px] font-semibold text-[#202126] dark:text-white">{item}</span>
              <span className={cn("rounded-full px-2 py-0.5 text-[10px] font-semibold", role.ringClass, role.iconClass)}>{index === 0 ? "Source" : "Review"}</span>
            </div>
          ))}
        </CounterpartPanel>
      </section>
    );
  }

  if (feature === "service-actions") {
    return (
      <section className="grid gap-3 lg:grid-cols-3" aria-label="Service action permissions">
        {operatingModel.actions.map((action) => (
          <CounterpartPanel
            key={action.title}
            role={role}
            icon={action.icon}
            eyebrow="Controlled tool"
            title={action.title}
            description={action.description}
        >
          <div className="space-y-2">
              {serviceActionChecks.map((item) => (
                <div key={item} className={cn("flex items-center gap-2 rounded-[8px] border px-3 py-2", roleInsetClass(role))}>
                  <CheckCircle2 className={cn("size-4", role.iconClass)} />
                  <span className="text-[11px] font-medium text-[#4B515C] dark:text-[#D8DEE8]">{item}</span>
                </div>
              ))}
            </div>
          </CounterpartPanel>
        ))}
      </section>
    );
  }

  if (feature === "service-channels") {
    return (
      <section className="grid gap-3" aria-label="Service channel rollout">
        <CounterpartPanel
          role={role}
          icon={Globe2}
          eyebrow="Intercom Fin style"
          title="Omnichannel rollout"
          description="Launch in the channels with the right confidence, tone, and escalation settings."
        >
          <div className="grid gap-2 md:grid-cols-3">
            {operatingModel.channels.map((channel) => (
              <FeatureRow key={channel.title} feature={channel} role={role} />
            ))}
          </div>
          <div className="mt-3 flex flex-wrap gap-1.5" aria-label="Supported service channels">
            {serviceChannelRollout.map((channel) => (
              <span key={channel} className={cn("rounded-full px-2.5 py-1 text-[10px] font-semibold", role.ringClass, role.iconClass)}>
                {channel}
              </span>
            ))}
          </div>
        </CounterpartPanel>
      </section>
    );
  }

  if (feature === "service-quality" || feature === "service-insights") {
    return (
      <section className="grid gap-3 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]" aria-label="Service quality and insights">
        <CounterpartPanel
          role={role}
          icon={CheckCircle2}
          eyebrow={feature === "service-quality" ? "Forethought style" : "Insight loop"}
          title={feature === "service-quality" ? "Quality supervisor" : "Conversation intelligence"}
          description={feature === "service-quality" ? "Every reply is scored for source coverage, policy fit, and escalation risk." : "Cluster unresolved questions, handoff reasons, and missing content into next best improvements."}
        >
          {(feature === "service-quality" ? serviceVerifiedQaRows : serviceInsightQaRows).map((row) => (
            <StatusRow key={row.label} label={row.label} value={row.value} role={role} />
          ))}
        </CounterpartPanel>
        <CounterpartPanel
          role={role}
          icon={Sparkles}
          eyebrow="Recommended work"
          title="Next improvements"
          description="Keep the agent improving without making support leads hunt through raw transcripts."
        >
          <div className="grid gap-2 sm:grid-cols-3">
            {serviceNextImprovements.map((item) => (
              <div key={item} className={cn("rounded-[8px] border p-3", roleInsetClass(role))}>
                <CheckCircle2 className={cn("size-4", role.iconClass)} />
                <p className="mt-2 text-[12px] font-semibold text-[#202126] dark:text-white">{item}</p>
              </div>
            ))}
          </div>
        </CounterpartPanel>
      </section>
    );
  }

  return (
    <section className="grid gap-3" aria-label="Service resolution workspace">
      <CounterpartPanel
        role={role}
        icon={TicketCheck}
        eyebrow="Zendesk / Ada style"
        title="Resolution queue"
        description="Route every support issue through answer, action, QA, and human handoff states."
      >
        <div className="grid gap-2 md:grid-cols-3">
          {serviceResolutionQueue.map((item) => (
            <div key={item.title} className={cn("rounded-[8px] border p-3", roleInsetClass(role))}>
              <div className="flex items-center justify-between gap-2">
                <h3 className="text-[12px] font-semibold text-[#202126] dark:text-white">{item.title}</h3>
                <span className={cn("size-2 rounded-full", role.accentClass)} />
              </div>
              <p className="mt-2 text-[11px] font-semibold text-[#4A505A] dark:text-[#DCE2EC]">{item.status}</p>
              <p className="mt-1 text-[11px] leading-4 text-[#747B87] dark:text-[#AEB4C0]">{item.detail}</p>
            </div>
          ))}
        </div>
        <div className={cn("mt-3 rounded-[8px] border p-3", roleInsetClass(role))}>
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className={cn("text-[11px] font-semibold uppercase", roleEyebrowClass(role))}>Zendesk-style verification</p>
              <p className="mt-1 text-[12px] font-semibold text-[#202126] dark:text-white">Only counted when the issue is actually resolved</p>
            </div>
            <span className={cn("rounded-full px-2 py-1 text-[10px] font-semibold", role.ringClass, role.iconClass)}>QA sampled</span>
          </div>
        </div>
        <div className="mt-3 grid gap-2 sm:grid-cols-2">
          <FeatureRow feature={operatingModel.knowledge[0]} role={role} />
          <FeatureRow feature={operatingModel.actions[0]} role={role} />
        </div>
      </CounterpartPanel>

      <CounterpartPanel
        role={role}
        icon={CheckCircle2}
        eyebrow="Forethought style"
        title="Quality supervisor"
        description="Every reply is scored for source coverage, policy fit, and escalation risk."
      >
        {serviceVerifiedQaRows.map((row) => (
          <StatusRow key={row.label} label={row.label} value={row.value} role={role} />
        ))}
        <div className="mt-3 space-y-2">
          {operatingModel.guardrails.map((guardrail) => (
            <FeatureRow key={guardrail.title} feature={guardrail} role={role} />
          ))}
        </div>
      </CounterpartPanel>

      <CounterpartPanel
        role={role}
        icon={Globe2}
        eyebrow="Intercom Fin style"
        title="Omnichannel rollout"
        description="Launch in the channels with the right confidence and escalation settings."
      >
        <div className="grid gap-2 md:grid-cols-3">
          {operatingModel.channels.map((channel) => (
            <FeatureRow key={channel.title} feature={channel} role={role} />
          ))}
        </div>
      </CounterpartPanel>
    </section>
  );
}

function SalesSdrSurface({
  feature,
  operatingModel,
  role,
}: {
  feature: AgentFeatureId;
  operatingModel: RoleOperatingModel;
  role: AgentBlueprint;
}) {
  if (feature === "sales-qualification" || feature === "sales-objections") {
    return (
      <section className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]" aria-label="Sales qualification workspace">
        <CounterpartPanel
          role={role}
          icon={feature === "sales-qualification" ? BriefcaseBusiness : ShieldCheck}
          eyebrow={feature === "sales-qualification" ? "Qualified Piper style" : "Approved playbook"}
          title={feature === "sales-qualification" ? "Qualification scorecard" : "Objection handling"}
          description={feature === "sales-qualification" ? "Collect fit, urgency, and buying context before a meeting is offered." : "Answer pricing, security, and migration concerns from approved sales guidance."}
        >
          <div className="grid gap-2 sm:grid-cols-3">
            {salesQualificationScores.map((item) => (
              <div key={item.label} className={cn("rounded-[8px] border p-3", roleInsetClass(role))}>
                <p className={cn("text-[11px] font-semibold uppercase", roleEyebrowClass(role))}>{item.label}</p>
                <p className="mt-2 text-[22px] font-semibold text-[#202126] dark:text-white">{item.value}</p>
              </div>
            ))}
          </div>
          <div className="mt-3 space-y-2">
            {(feature === "sales-qualification" ? operatingModel.actions : salesObjectionRows).map((item) => (
              <FeatureRow key={item.title} feature={item} role={role} />
            ))}
          </div>
          {feature === "sales-qualification" ? (
            <div className="mt-3 grid gap-1.5 sm:grid-cols-4" aria-label="Visitor intelligence signals">
              {salesVisitorIntelligenceSignals.map((item) => (
                <span key={item} className={cn("rounded-full border px-2 py-1 text-center text-[10px] font-semibold", roleInsetClass(role), roleEyebrowClass(role))}>
                  {item}
                </span>
              ))}
            </div>
          ) : null}
        </CounterpartPanel>
        <CounterpartPanel
          role={role}
          icon={MessageSquareText}
          eyebrow="Conversation blueprint"
          title="Buyer context"
          description="The agent keeps the conversation focused and compact."
        >
          <RoleConversationPreview operatingModel={operatingModel} role={role} />
        </CounterpartPanel>
      </section>
    );
  }

  if (feature === "sales-booking" || feature === "sales-crm") {
    return (
      <section className="grid gap-3 lg:grid-cols-2" aria-label="Sales booking and CRM workspace">
        <CounterpartPanel
          role={role}
          icon={feature === "sales-booking" ? CalendarClock : Database}
          eyebrow="HubSpot / Salesforce style"
          title={feature === "sales-booking" ? "Meeting router" : "CRM handoff builder"}
          description={feature === "sales-booking" ? "Show the right slots only after the lead reaches a sales-ready threshold." : "Create structured handoffs with source pages, objections, qualification, and next steps."}
        >
          {feature === "sales-booking" ? (
            <div className="grid grid-cols-2 gap-2">
              {salesMeetingSlots.map((slot, index) => (
                <VerevonButton
                  key={slot}
                  variant={index === 1 ? "primary" : "secondary"}
                  radius="sm"
                  aria-pressed={index === 1}
                  className={cn("px-3 text-[12px] font-semibold", controlFocusClass)}
                >
                  {slot}
                </VerevonButton>
              ))}
              <div className={cn("col-span-2 rounded-[8px] border px-3 py-2 text-[11px] font-medium text-[#4B515C] dark:text-[#D8DEE8]", roleInsetClass(role))}>
                Uses the record owner calendar first; sends a meeting link if no live slot is available.
              </div>
            </div>
          ) : (
            <div className="space-y-2">
              {salesCrmHandoffItems.map((item) => (
                <div key={item} className={cn("rounded-[8px] border px-3 py-2 text-[12px] font-medium text-[#4B515C] dark:text-[#D8DEE8]", roleInsetClass(role))}>{item}</div>
              ))}
              <div className="grid grid-cols-2 gap-1.5 pt-1" aria-label="Breeze-style research context">
                {salesBreezeResearch.map((item) => (
                  <span key={item} className={cn("rounded-full px-2 py-1 text-center text-[10px] font-semibold", role.ringClass, role.iconClass)}>
                    {item}
                  </span>
                ))}
              </div>
            </div>
          )}
        </CounterpartPanel>
        <CounterpartPanel
          role={role}
          icon={Split}
          eyebrow="Routing rules"
          title="Owner assignment"
          description="Route by segment, region, account owner, and urgency."
        >
          <div className="space-y-2">
            {operatingModel.channels.map((channel) => (
              <FeatureRow key={channel.title} feature={channel} role={role} />
            ))}
          </div>
        </CounterpartPanel>
      </section>
    );
  }

  if (feature === "sales-insights") {
    return (
      <section className="grid gap-3 md:grid-cols-2 xl:grid-cols-4" aria-label="Sales insights workspace">
        {salesInsightMetrics.map((metric) => (
          <CounterpartPanel key={metric.label} role={role} icon={BarChart3} eyebrow="Pipeline insight" title={metric.label} description={metric.detail}>
            <p className="text-[34px] font-semibold leading-none text-[#202126] dark:text-white">{metric.value}</p>
            <StatusRow label="Signal" value="Runtime gated" role={role} />
          </CounterpartPanel>
        ))}
      </section>
    );
  }

  return (
    <section className="grid gap-3" aria-label="Sales AI SDR workspace">
      <CounterpartPanel
        role={role}
        icon={Zap}
        eyebrow="Qualified Piper style"
        title="AI SDR journey"
        description="Detect intent, qualify in chat, book the right owner, and write the CRM handoff."
      >
        <div className="grid gap-3 lg:grid-cols-[190px_minmax(0,1fr)]">
          <div className={cn("rounded-[8px] border p-3", roleInsetClass(role))}>
            <p className={cn("text-[11px] font-semibold uppercase", roleEyebrowClass(role))}>Intent stream</p>
            <div className="mt-3 space-y-2">
              {salesIntentSignals.map((signal) => (
                <div key={signal} className="flex items-center gap-2 text-[11px] font-medium text-[#4B515C] dark:text-[#D8DEE8]">
                  <span className={cn("size-1.5 rounded-full", role.accentClass)} />
                  {signal}
                </div>
              ))}
            </div>
            <div className="mt-3 grid grid-cols-2 gap-1.5" aria-label="Piper engagement modes">
              {salesEngagementModes.map((mode) => (
                <span key={mode} className={cn("rounded-full px-2 py-1 text-center text-[10px] font-semibold", role.ringClass, role.iconClass)}>
                  {mode}
                </span>
              ))}
            </div>
          </div>
          <div className={cn("rounded-[8px] border p-3 shadow-sm", rolePanelClass(role))}>
            <div className="flex items-start justify-between gap-3">
              <div>
                <p className={cn("text-[11px] font-semibold uppercase", roleEyebrowClass(role))}>Qualification</p>
                <h3 className="mt-1 text-[18px] font-semibold text-[#202126] dark:text-white">Sales-ready rules</h3>
              </div>
              <span className={cn("rounded-full px-2 py-1 text-[10px] font-semibold", role.ringClass, role.iconClass)}>Route gated</span>
            </div>
            <div className="mt-3 grid gap-2 sm:grid-cols-3">
              {salesLeadTags.map((item) => (
                <span key={item} className="rounded-full border border-[#E3E4E8] px-2.5 py-1 text-[11px] font-medium text-[#4B515C] dark:border-[#2B2D33] dark:text-[#D8DEE8]">
                  {item}
                </span>
              ))}
            </div>
            <p className="mt-3 text-[12px] leading-5 text-[#68707B] dark:text-[#AEB4C0]">
              The agent asks two discovery questions before showing calendar slots, then sends the CRM handoff with page history and objections.
            </p>
          </div>
        </div>
      </CounterpartPanel>

      <CounterpartPanel
        role={role}
        icon={CalendarClock}
        eyebrow="HubSpot / Salesforce style"
        title="Meeting + CRM"
        description="Booking and handoff are treated as controlled sales actions."
      >
        <div className="grid grid-cols-2 gap-2">
          {salesMeetingSlots.map((slot, index) => (
            <VerevonButton
              key={slot}
              variant={index === 1 ? "primary" : "secondary"}
              size="sm"
              radius="sm"
              aria-pressed={index === 1}
              className={cn("px-3 text-[12px] font-semibold", controlFocusClass)}
            >
              {slot}
            </VerevonButton>
          ))}
        </div>
        <div className="mt-3 space-y-2">
          {operatingModel.actions.map((action) => (
            <FeatureRow key={action.title} feature={action} role={role} />
          ))}
        </div>
      </CounterpartPanel>
    </section>
  );
}

function EcommerceCommerceSurface({
  feature,
  operatingModel,
  role,
}: {
  feature: AgentFeatureId;
  operatingModel: RoleOperatingModel;
  role: AgentBlueprint;
}) {
  if (feature === "commerce-support") {
    return (
      <section className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]" aria-label="Ecommerce support and orders workspace">
        <CounterpartPanel
          role={role}
          icon={TicketCheck}
          eyebrow="Gorgias Support Agent style"
          title="Orders, returns, and subscriptions"
          description="Resolve post-purchase work with live order state and scoped support actions."
        >
          <div className="grid gap-2 sm:grid-cols-3">
            {ecommerceSupportRequests.map((item) => (
              <div key={item.title} className={cn("rounded-[8px] border p-3", roleInsetClass(role))}>
                <p className="text-[12px] font-semibold text-[#202126] dark:text-white">{item.title}</p>
                <p className="mt-2 text-[11px] text-[#68707B] dark:text-[#AEB4C0]">{item.detail}</p>
              </div>
            ))}
          </div>
          <div className="mt-3 grid gap-2 md:grid-cols-3">
            {operatingModel.actions.slice(1).map((action) => (
              <FeatureRow key={action.title} feature={action} role={role} />
            ))}
          </div>
        </CounterpartPanel>
        <CounterpartPanel
          role={role}
          icon={Search}
          eyebrow="Store state requirements"
          title="Order lookup"
          description="The agent checks status before promising refunds, delivery dates, or subscription changes."
        >
          <StatusRow label="Order data" value="Connect" role={role} />
          <StatusRow label="Return policy" value="Review" role={role} />
          <StatusRow label="Subscription action" value="Scope" role={role} />
        </CounterpartPanel>
      </section>
    );
  }

  if (feature === "commerce-product-finder") {
    return (
      <section className="grid gap-3 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]" aria-label="Ecommerce product finder workspace">
        <CounterpartPanel
          role={role}
          icon={Search}
          eyebrow="Rep AI Guided Search style"
          title="Guided product finder"
          description="Ask one useful question, filter with catalog attributes, and explain the best matches."
        >
          <div className={cn("rounded-[8px] border p-3", roleInsetClass(role))}>
            <p className="text-[12px] font-semibold text-[#202126] dark:text-white">What matters most for winter runs?</p>
            <div className="mt-2 flex flex-wrap gap-1.5">
              {ecommerceFinderChips.map((chip) => (
                <span key={chip} className="rounded-full border border-[#DDE0E5] bg-white px-2.5 py-1 text-[10px] font-semibold text-[#4B515C] dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#DCE2EC]">
                  {chip}
                </span>
              ))}
            </div>
          </div>
          <div className="mt-3 grid gap-2 md:grid-cols-3">
            {ecommerceProductRecommendations.map((product) => (
              <div key={product.name} className={cn("rounded-[8px] border p-2 shadow-sm", rolePanelClass(role))}>
                <div className="grid aspect-[4/3] place-items-center rounded-[7px] bg-[linear-gradient(135deg,#EAF9DF,#D7E9C8)] dark:bg-[linear-gradient(135deg,#1F3326,#253B2D)]">
                  <span className="h-4 w-14 rounded-full bg-white/80 shadow-sm dark:bg-white/20" />
                </div>
                <h3 className="mt-2 text-[11px] font-semibold text-[#202126] dark:text-white">{product.name}</h3>
                <p className="mt-0.5 text-[10px] text-[#68707B] dark:text-[#AEB4C0]">{product.fit}</p>
                <p className="mt-1 text-[12px] font-semibold text-[#202126] dark:text-white">{product.price}</p>
              </div>
            ))}
          </div>
        </CounterpartPanel>
        <CounterpartPanel
          role={role}
          icon={BarChart3}
          eyebrow="Shopper intelligence"
          title="Intent profile"
          description="Use browsing behavior, budget, and cart context to keep recommendations relevant."
        >
          <FeatureRow feature={operatingModel.knowledge[1]} role={role} />
          <div className="mt-2">
            <FeatureRow feature={operatingModel.guardrails[0]} role={role} />
          </div>
        </CounterpartPanel>
      </section>
    );
  }

  if (feature === "commerce-cart") {
    return (
      <section className="grid gap-3 lg:grid-cols-3" aria-label="Ecommerce cart recovery workspace">
        {ecommerceCartRecoveryCards.map((item) => (
          <CounterpartPanel key={item.title} role={role} icon={Rocket} eyebrow="Cart recovery" title={item.title} description={item.detail}>
            <span className={cn("inline-flex rounded-full px-3 py-1 text-[11px] font-semibold", role.ringClass, role.iconClass)}>{item.value}</span>
          </CounterpartPanel>
        ))}
      </section>
    );
  }

  if (feature === "commerce-brand") {
    return (
      <section className="grid gap-3 lg:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)]" aria-label="Ecommerce brand voice workspace">
        <CounterpartPanel
          role={role}
          icon={MessageSquareText}
          eyebrow="Siena AI Personas style"
          title="AI Persona + social"
          description="Keep autonomous commerce replies on-brand across chat, email, WhatsApp, Instagram, and social comments."
        >
          <StatusRow label="Brand voice match" value="Review" role={role} />
          <StatusRow label="Social response quality" value="Policy" role={role} />
          <StatusRow label="Voice of customer coverage" value="Waiting" role={role} />
        </CounterpartPanel>
        <CounterpartPanel
          role={role}
          icon={Globe2}
          eyebrow="Channel rules"
          title="Autonomous CX channels"
          description="Different channels can have different tone, length, escalation, and response permissions."
        >
          <div className="grid gap-2 sm:grid-cols-3">
            {operatingModel.channels.map((channel) => (
              <FeatureRow key={channel.title} feature={channel} role={role} />
            ))}
          </div>
        </CounterpartPanel>
      </section>
    );
  }

  if (feature === "commerce-store") {
    return (
      <section className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]" aria-label="Ecommerce store command center">
        <CounterpartPanel
          role={role}
          icon={Settings2}
          eyebrow="Shopify Sidekick style"
          title="Store command center"
          description="Ask in plain language, review generated store actions, then apply with approval."
        >
          <div className={cn("rounded-[8px] border p-3", roleInsetClass(role))}>
            <p className={cn("text-[11px] font-semibold uppercase", roleEyebrowClass(role))}>Command</p>
            <div className="mt-2 rounded-[8px] border border-[#E1E3E8] bg-white px-3 py-2 text-[12px] text-[#343A43] shadow-sm dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#E7EBF1]">
              Create a winter-running product finder and tag low-stock items.
            </div>
            <div className="mt-3 space-y-2">
              {ecommerceStoreActionPlan.map((item) => (
                <div key={item} className="flex items-center gap-2 text-[11px] font-medium text-[#4B515C] dark:text-[#D8DEE8]">
                  <CheckCircle2 className={cn("size-3.5", role.iconClass)} />
                  {item}
                </div>
              ))}
              <div className="grid grid-cols-3 gap-1.5 pt-1" aria-label="Sidekick task types">
                {ecommerceSidekickTasks.map((item) => (
                  <span key={item} className="rounded-full border border-[#DDE0E5] bg-white px-2 py-1 text-center text-[9px] font-semibold text-[#4B515C] dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#DCE2EC]">
                    {item}
                  </span>
                ))}
              </div>
            </div>
            <div className="mt-3 flex gap-2">
              <VerevonButton variant="primary" size="xs" radius="pill" className={cn("flex-1 px-3 font-semibold", controlFocusClass)}>Review</VerevonButton>
              <VerevonButton variant="secondary" size="xs" radius="pill" className={cn("flex-1 px-3 font-semibold", controlFocusClass)}>Apply</VerevonButton>
            </div>
          </div>
        </CounterpartPanel>
        <CounterpartPanel
          role={role}
          icon={CheckCircle2}
          eyebrow="Approval rail"
          title="Action safety"
          description="Store changes stay reviewable and scoped before anything writes to Shopify."
        >
          <FeatureRow feature={operatingModel.guardrails[0]} role={role} />
          <div className="mt-2">
            <FeatureRow feature={operatingModel.guardrails[1]} role={role} />
          </div>
        </CounterpartPanel>
      </section>
    );
  }

  if (feature === "commerce-insights") {
    return (
      <section className="grid gap-3 lg:grid-cols-3" aria-label="Ecommerce shopper insights workspace">
        {ecommerceInsightCards.map((metric) => (
          <CounterpartPanel key={metric.title} role={role} icon={BarChart3} eyebrow="Shopper insight" title={metric.title} description={metric.detail}>
            <p className="text-[32px] font-semibold leading-none text-[#202126] dark:text-white">{metric.value}</p>
          </CounterpartPanel>
        ))}
      </section>
    );
  }

  return (
    <section className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_330px]" aria-label="Ecommerce shopping assistant workspace">
      <CounterpartPanel
        role={role}
        icon={ShoppingBag}
        eyebrow="Gorgias style"
        title="Shopping Assistant + Support Agent"
        description="Pre-purchase product discovery and post-purchase order work live in one commerce agent."
      >
        <div className="grid gap-2 md:grid-cols-2">
          {ecommerceAssistantModules.map((skill) => (
            <div key={skill.title} className={cn("rounded-[8px] border p-3", roleInsetClass(role))}>
              <div className="flex items-center gap-2">
                <span className={cn("grid size-8 place-items-center rounded-[7px] text-white", role.accentClass)}>
                  <skill.icon className="size-4" />
                </span>
                <h3 className="text-[13px] font-semibold text-[#202126] dark:text-white">{skill.title}</h3>
              </div>
              <p className="mt-2 text-[11px] leading-4 text-[#68707B] dark:text-[#AEB4C0]">{skill.detail}</p>
            </div>
          ))}
        </div>
        <div className="mt-3 grid gap-2 md:grid-cols-3">
          {operatingModel.actions.map((action) => (
            <FeatureRow key={action.title} feature={action} role={role} />
          ))}
        </div>
      </CounterpartPanel>
      <CounterpartPanel
        role={role}
        icon={MessageSquareText}
        eyebrow="AI FAQ"
        title="Product-page questions"
        description="Proactively answer sizing, material, shipping, and return questions where shoppers hesitate."
      >
        {ecommerceProductQuestions.map((question) => (
          <div key={question} className={cn("mt-2 rounded-[8px] border px-3 py-2 text-[12px] font-medium text-[#4B515C] first:mt-0 dark:text-[#D8DEE8]", roleInsetClass(role))}>
            {question}
          </div>
        ))}
        <div className="mt-3 flex flex-wrap gap-1.5" aria-label="Proactive quick replies">
          {ecommerceQuickReplies.map((reply) => (
            <span key={reply} className={cn("rounded-full px-2.5 py-1 text-[10px] font-semibold", role.ringClass, role.iconClass)}>
              {reply}
            </span>
          ))}
        </div>
      </CounterpartPanel>
    </section>
  );
}
