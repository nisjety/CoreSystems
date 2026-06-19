import Image from "next/image";
import type { LucideIcon } from "lucide-react";
import {
  ArrowDown,
  ArrowRight,
  Bot,
  BookOpen,
  CheckCircle2,
  Database,
  FileText,
  Globe2,
  Inbox,
  Lock,
  MessageSquare,
  RotateCcw,
  ScrollText,
  Send,
  ShieldCheck,
  UserCheck,
  Workflow,
} from "lucide-react";
import {
  actionTimeline,
  conversations,
  footerColumns,
  proofPoints,
  siteConfig,
  sourceTrace,
  trustPillars,
  workflows,
} from "./content";

const proofIcons = [Bot, ShieldCheck, Database] as const;
const workflowIcons = [MessageSquare, BookOpen, Workflow] as const;
const trustIcons = [UserCheck, ScrollText, RotateCcw, Globe2, Lock] as const;

function IconBadge({
  icon: Icon,
  tone = "neutral",
}: {
  icon: LucideIcon;
  tone?: "neutral" | "blue" | "orange" | "violet";
}) {
  return (
    <span className={`icon-badge icon-badge--${tone}`} aria-hidden="true">
      <Icon size={22} strokeWidth={1.7} />
    </span>
  );
}

function CtaLink({
  children,
  href,
  variant = "primary",
}: {
  children: string;
  href: string;
  variant?: "primary" | "secondary";
}) {
  return (
    <a className={`cta cta--${variant}`} href={href}>
      <span>{children}</span>
      <ArrowRight size={16} strokeWidth={1.8} aria-hidden="true" />
    </a>
  );
}

export default function Home() {
  return (
    <main>
      <header className="site-header" aria-label="Primary navigation">
        <a className="brand" href="#top" aria-label="Velion home">
          {siteConfig.name}
        </a>
        <nav className="nav-links">
          {siteConfig.navItems.map((item) => (
            <a href={`#${item.toLowerCase()}`} key={item}>
              {item}
            </a>
          ))}
        </nav>
        <CtaLink href="#demo">{siteConfig.primaryCta}</CtaLink>
      </header>

      <section className="hero" id="top">
        <div className="hero-copy">
          <span className="section-index" aria-hidden="true">
            001
          </span>
          <h1>{siteConfig.heroTitle}</h1>
          <p>{siteConfig.heroBody}</p>
          <div className="hero-actions" aria-label="Hero actions">
            <CtaLink href="#demo">{siteConfig.primaryCta}</CtaLink>
            <CtaLink href="#product" variant="secondary">
              {siteConfig.secondaryCta}
            </CtaLink>
          </div>
          <div className="proof-row" aria-label="Velion principles">
            {proofPoints.map((point, index) => {
              const Icon = proofIcons[index];

              return (
                <div className="proof-item" key={point.title}>
                  {Icon ? <Icon size={24} strokeWidth={1.6} /> : null}
                  <span>{point.title}</span>
                </div>
              );
            })}
          </div>
        </div>

        <div className="hero-media" aria-hidden="true">
          <Image
            src="/images/velion-mesh-hero.png"
            alt=""
            fill
            priority
            sizes="(max-width: 900px) 100vw, 58vw"
          />
        </div>

        <a className="scroll-cue" href="#product" aria-label="Scroll to product">
          <ArrowDown size={18} strokeWidth={1.7} />
        </a>
      </section>

      <section className="workbench-section" id="product">
        <div className="section-heading">
          <span className="section-index" aria-hidden="true">
            002
          </span>
          <h2>Your workbench. All in one view.</h2>
          <p>
            From first message to final resolution, Velion keeps context,
            sources, approvals, and actions in sync.
          </p>
        </div>

        <div className="workbench" aria-label="Velion workbench preview">
          <aside className="rail" aria-label="Workspace areas">
            <span className="rail-mark">V</span>
            {["Inbox", "Knowledge", "Workflows", "Agents", "Settings"].map(
              (label, index) => (
                <span
                  className={index === 0 ? "rail-item active" : "rail-item"}
                  key={label}
                >
                  {label}
                </span>
              ),
            )}
            <span className="rail-avatar" aria-hidden="true" />
          </aside>

          <section className="inbox-panel" aria-label="Inbox">
            <div className="panel-header">
              <Inbox size={18} strokeWidth={1.8} />
              <span>Inbox</span>
            </div>
            {conversations.map((conversation) => (
              <article className="conversation-row" key={conversation.subject}>
                <div>
                  <strong>{conversation.channel}</strong>
                  <span>{conversation.subject}</span>
                </div>
                <time>{conversation.time}</time>
              </article>
            ))}
            <a className="panel-link" href="#demo">
              View all conversations
              <ArrowRight size={14} strokeWidth={1.7} />
            </a>
          </section>

          <section className="answer-panel" aria-label="Velion draft answer">
            <div className="question-card">
              <span>Customer question</span>
              <h3>Hvor er bestillingen min?</h3>
            </div>
            <div className="summary-card">
              <span>AI summary</span>
              <p>
                Customer asks for order status. Package was shipped May 14 and
                is expected May 16.
              </p>
            </div>
            <div className="draft-card">
              <span>Velion draft</span>
              <p>
                Hei. Bestillingen din er sendt og forventet levert 16. mai. Du
                kan spore pakken her.
              </p>
              <div className="draft-actions">
                <button type="button">
                  <Send size={14} strokeWidth={1.8} />
                  Send answer
                </button>
                <button type="button" className="button-secondary">
                  Edit
                </button>
                <button type="button" className="button-secondary">
                  Note
                </button>
              </div>
            </div>
          </section>

          <aside className="context-panel" aria-label="Source and approval">
            <div className="source-card">
              <div className="panel-header">
                <FileText size={17} strokeWidth={1.8} />
                <span>Knowledge source trace</span>
              </div>
              {sourceTrace.map((source) => (
                <div className="source-row" key={source.title}>
                  <div>
                    <strong>{source.title}</strong>
                    <span>{source.path}</span>
                  </div>
                  <em>{source.status}</em>
                </div>
              ))}
            </div>

            <div className="approval-card">
              <div className="panel-header">
                <ShieldCheck size={17} strokeWidth={1.8} />
                <span>Approval queue</span>
              </div>
              <p>Action: send answer to customer.</p>
              <div className="approval-actions">
                <button type="button">Approve</button>
                <button type="button" className="button-secondary">
                  Reject
                </button>
              </div>
            </div>

            <div className="timeline-card">
              <span>Action timeline</span>
              <ol>
                {actionTimeline.map((item) => (
                  <li key={item}>
                    <CheckCircle2 size={14} strokeWidth={1.8} />
                    {item}
                  </li>
                ))}
              </ol>
            </div>
          </aside>
        </div>
      </section>

      <section className="workflow-section" id="workflows">
        <div className="section-heading">
          <span className="section-index" aria-hidden="true">
            003
          </span>
          <h2>Built for customer experience teams</h2>
        </div>

        <div className="workflow-list">
          {workflows.map((workflow, index) => {
            const Icon = workflowIcons[index] ?? Workflow;
            const tone = index === 1 ? "orange" : index === 2 ? "violet" : "blue";

            return (
              <article className={`workflow-row workflow-row--${tone}`} key={workflow.title}>
                <span className="workflow-number">{workflow.number}</span>
                <IconBadge icon={Icon} tone={tone} />
                <div className="workflow-copy">
                  <h3>{workflow.title}</h3>
                  <p>{workflow.body}</p>
                </div>
                <div className="workflow-steps" aria-label={`${workflow.title} steps`}>
                  {workflow.steps.map((step, stepIndex) => (
                    <span key={step}>
                      {step}
                      {stepIndex < workflow.steps.length - 1 ? (
                        <ArrowRight size={13} strokeWidth={1.7} aria-hidden="true" />
                      ) : null}
                    </span>
                  ))}
                </div>
              </article>
            );
          })}
        </div>
      </section>

      <section className="trust-section" id="trust">
        <div className="section-heading">
          <span className="section-index" aria-hidden="true">
            004
          </span>
          <h2>Trusted by design</h2>
        </div>

        <div className="trust-grid">
          {trustPillars.map((pillar, index) => {
            const Icon = trustIcons[index] ?? ShieldCheck;

            return (
              <article className="trust-item" key={pillar.title}>
                <IconBadge icon={Icon} />
                <h3>{pillar.title}</h3>
                <p>{pillar.body}</p>
              </article>
            );
          })}
        </div>
      </section>

      <section className="final-cta" id="demo">
        <div>
          <span className="section-index" aria-hidden="true">
            005
          </span>
          <h2>See Velion in action</h2>
          <p>
            Discover how Velion can elevate your customer experience and scale
            with confidence.
          </p>
        </div>
        <div className="final-actions">
          <CtaLink href="mailto:hello@velion.no">{siteConfig.primaryCta}</CtaLink>
          <CtaLink href="mailto:hello@velion.no" variant="secondary">
            Contact sales
          </CtaLink>
        </div>
      </section>

      <footer className="site-footer">
        <div className="footer-brand">
          <strong>{siteConfig.name}</strong>
          <p>
            AI-first customer experience. Human-approved. Always.
          </p>
          <span>Made in Norway</span>
        </div>
        <nav className="footer-nav" aria-label="Footer navigation">
          {footerColumns.map((column) => (
            <div key={column.title}>
              <h3>{column.title}</h3>
              {column.links.map((link) => (
                <a href="#top" key={link}>
                  {link}
                </a>
              ))}
            </div>
          ))}
        </nav>
        <form className="footer-form">
          <label htmlFor="email">Stay updated</label>
          <div>
            <input id="email" name="email" placeholder="you@company.com" type="email" />
            <button type="button" aria-label="Submit email">
              <ArrowRight size={16} strokeWidth={1.8} />
            </button>
          </div>
        </form>
        <div className="footer-bottom">
          <span>© 2026 Velion AS. All rights reserved.</span>
          <span>Privacy · Terms · Cookie settings</span>
        </div>
      </footer>
    </main>
  );
}
