import { ArrowButton } from "@/components/ui/buttons/ArrowButton";
import { VisualPanel, type VisualPanelVariant } from "./VisualPanel";

const cards: Array<{
  href: string;
  link: string;
  number: string;
  title: string;
  text: string;
  visual: VisualPanelVariant;
}> = [
  {
    href: "#partnership",
    link: "Ask Velion",
    number: "001",
    title: "Create the chatbot",
    text: "Connect sources, crawl the site, inspect what Velion learned, and publish the first customer-facing assistant.",
    visual: "craft",
  },
  {
    href: "#technology",
    link: "Route work",
    number: "002",
    title: "Run the queue",
    text: "Draft replies for email, chat, and social, then route uncertain cases to the right human before anything leaves.",
    visual: "surface",
  },
  {
    href: "#company",
    link: "Trust layer",
    number: "003",
    title: "Approve action",
    text: "Policies, macros, handoff rules, and workflow steps stay reversible, auditable, and available manually in the UI.",
    visual: "mark",
  },
];

export function FeatureCardsSection() {
  return (
    <section className="velion-cards" id="yachts">
      <div className="velion-cards__grid">
        {cards.map((card) => (
          <article className="velion-card" key={card.number}>
            <VisualPanel label={card.title} variant={card.visual} />
            <p>{card.number}</p>
            <h2>{card.title}</h2>
            <span className="velion-card__text">{card.text}</span>
            <ArrowButton href={card.href} variant="muted">
              {card.link}
            </ArrowButton>
          </article>
        ))}
      </div>
    </section>
  );
}
