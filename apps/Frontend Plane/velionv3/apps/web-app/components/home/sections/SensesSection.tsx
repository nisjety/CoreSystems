import { VisualPanel } from "./VisualPanel";

export function SensesSection() {
  return (
    <section className="velion-section velion-section--reverse velion-section--senses">
      <VisualPanel className="velion-section__visual" label="Acoustic experience profile" variant="open" />
      <div className="velion-section__copy">
        <span className="velion-copy-rule fade-out-top" data-fade-out-top aria-hidden="true" />
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          Elevate your senses
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            The acoustic user journey on THE ICON is composed by two-time Academy Award winner Hans Zimmer. It extends
            the successful cooperation between Hans Zimmer and BMW towards marine mobility. It comprises pleasant,
            intuitive functional sounds in addition to a modern and immersive driving experience. Hans Zimmer was
            inspired for his composing by the environment itself - The sea is beautiful and deserves a beautiful sound,
            too.
          </p>
          <p>
            Dolby Atmos removes the boundaries of creative expression for artists to create a spatial sound experience
            that puts you at the center of your entertainment. THE ICON, as first-ever Dolby Atmos certified yacht in
            this class, immerse you in a world of acoustic depth, clarity and detail, while flying above the water.
          </p>
          <p>The perfect acoustic stage for the exclusive composing of Hans Zimmer.</p>
        </div>
      </div>
    </section>
  );
}
