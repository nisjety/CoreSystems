import { ArrowButton } from "@/components/ui/buttons/ArrowButton";

const stats = [
  { label: "Range at 24 kn", unit: "nm", value: "› 50" },
  { label: "Maximum speed", unit: "kn", value: "30" },
  { label: "Battery capacity", unit: "KWh", value: "240" },
];

export function TechnologySection() {
  return (
    <section className="velion-technology" id="technology">
      <div className="velion-technology__visual" aria-hidden="true">
        <span className="velion-technology__body" />
        <span className="velion-technology__foil velion-technology__foil--one" />
        <span className="velion-technology__foil velion-technology__foil--two" />
        <span className="velion-technology__wake" />
      </div>

      <div className="velion-technology__copy">
        <span className="velion-copy-rule fade-out-top" data-fade-out-top aria-hidden="true" />
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          Pioneering technology
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            Discover VELION&apos;s advanced technology, a synergy of expertise from global leaders in mobility, naval
            architecture, marine engineering, and design.
          </p>
        </div>
        <div className="velion-technology__stats">
          {stats.map((stat) => (
            <div key={stat.label}>
              <strong>
                {stat.value} <sup>{stat.unit}</sup>
              </strong>
              <span>{stat.label}</span>
            </div>
          ))}
        </div>
        <ArrowButton href="#company" variant="dark">
          Learn More
        </ArrowButton>
      </div>
    </section>
  );
}
