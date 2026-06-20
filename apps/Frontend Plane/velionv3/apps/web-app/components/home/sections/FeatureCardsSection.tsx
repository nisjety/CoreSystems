import { ArrowButton } from "@/components/ui/buttons/ArrowButton";
import { VisualPanel, type VisualPanelVariant } from "./VisualPanel";

const cards: Array<{
  href: string;
  link: string;
  number: string;
  title: string;
  visual: VisualPanelVariant;
}> = [
  {
    href: "#yachts",
    link: "Yachts",
    number: "001",
    title: "The ultimate flow state",
    visual: "craft",
  },
  {
    href: "#technology",
    link: "Technology",
    number: "002",
    title: "Pioneering technology",
    visual: "surface",
  },
  {
    href: "#company",
    link: "About",
    number: "003",
    title: "History in the making",
    visual: "mark",
  },
];

export function FeatureCardsSection() {
  return (
    <section className="velion-cards" id="yachts">
      <div className="velion-cards__grid">
        {cards.map((card, index) => (
          <article
            className="velion-card velion-reveal"
            key={card.number}
            style={{ "--delay": `${index * 90}ms` } as React.CSSProperties}
          >
            <VisualPanel label={card.title} variant={card.visual} />
            <p>{card.number}</p>
            <h2>{card.title}</h2>
            <ArrowButton href={card.href} variant="muted">
              {card.link}
            </ArrowButton>
          </article>
        ))}
      </div>
    </section>
  );
}
