import { ArrowButton } from "@/components/ui/buttons/ArrowButton";
import { VisualPanel, type VisualPanelVariant } from "./VisualPanel";

const workflowCards: Array<{
  copy: string;
  link: string;
  title: string;
  visual: VisualPanelVariant;
}> = [
  {
    copy: "Velion watches queues, sources, and customer signals so the team can act before service gaps become churn.",
    link: "Ask Velion",
    title: "Stop losing customers to downtime.",
    visual: "craft",
  },
  {
    copy: "Turn scattered tools into one source-backed work surface for drafting replies, routing cases, and preparing next actions.",
    link: "Route work",
    title: "Replacing Tab-Heavy Workflows",
    visual: "surface",
  },
  {
    copy: "Permissions, approvals, rollback paths, and audit history stay visible whenever Velion proposes or executes work.",
    link: "Trust layer",
    title: "Built for security-first teams.",
    visual: "mark",
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
            <h1>{card.title}</h1>
            <p>{card.copy}</p>
            <ArrowButton href="#product-loop" variant="muted">
              {card.link}
            </ArrowButton>
          </article>
        ))}
      </div>

    </section>
  );
}
