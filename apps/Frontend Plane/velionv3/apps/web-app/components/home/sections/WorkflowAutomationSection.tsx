import { ArrowButton } from "@/components/ui/buttons/ArrowButton";
import { VisualPanel, type VisualPanelVariant } from "./VisualPanel";

const workflowCards: Array<{
  action: string;
  copy: string;
  link: string;
  number: string;
  title: string;
  visual: VisualPanelVariant;
}> = [
  {
    action: "Monitor",
    copy: "Connect sources, crawl the site, inspect what Velion learned, and watch customer queues before work stalls.",
    link: "Ask Velion",
    number: "001",
    title: "Stop losing customers to downtime.",
    visual: "craft",
  },
  {
    action: "Brief",
    copy: "Turn scattered tabs into one source-backed brief, then draft replies for email, chat, social, and sales assistance.",
    link: "Route work",
    number: "002",
    title: "Replacing Tab-Heavy Workflows",
    visual: "surface",
  },
  {
    action: "Approve",
    copy: "Policies, macros, handoff rules, and risky workflow steps pause for role checks and explicit human approval.",
    link: "Trust layer",
    number: "003",
    title: "Built for security-first teams.",
    visual: "mark",
  },
  {
    action: "Audit",
    copy: "Keep integrations, permissions, rollback paths, execution history, and manual equivalents visible after every action.",
    link: "View audit path",
    number: "004",
    title: "Permissions, approvals, integrations, rollback.",
    visual: "detail",
  },
];

export function WorkflowAutomationSection() {
  return (
    <section className="velion-workflows" id="workflows">
      <div className="velion-workflows__header">
        <span className="velion-copy-label fade-out-top" data-fade-out-top>
          Operational workflows
        </span>
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          Concrete automation, controlled by humans.
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            The useful work is not a chat demo. Velion monitors signals, prepares actions, requests approval, and keeps
            the evidence trail available for every team that touches the customer.
          </p>
        </div>
      </div>

      <div className="velion-workflows__grid">
        {workflowCards.map((card) => (
          <article className="velion-workflows__card" key={card.title}>
            <VisualPanel className="velion-workflows__visual" label={card.title} variant={card.visual} />
            <div className="velion-workflows__meta">
              <span>{card.number}</span>
              <strong>{card.action}</strong>
            </div>
            <h2>{card.title}</h2>
            <p>{card.copy}</p>
            <ArrowButton href="#product-loop" variant="muted">
              {card.link}
            </ArrowButton>
          </article>
        ))}
      </div>

      <ArrowButton href="#trust" variant="muted">
        Watch the trust layer
      </ArrowButton>
    </section>
  );
}
