import { ArrowButton } from "@/components/ui/buttons/ArrowButton";
import { VelionCircuitDiagram } from "./VelionCircuitDiagram";

const stats = [
  { label: "AI worker loop", unit: "steps", value: "4" },
  { label: "Manual parity", unit: "%", value: "100" },
  { label: "Risky actions", unit: "approved", value: "HITL" },
];

export function TechnologySection() {
  return (
    <section className="velion-technology" id="technology">
      <div className="velion-technology__visual" aria-hidden="true">
        <div className="velion-technology__diagram-shell">
          <VelionCircuitDiagram />
        </div>
      </div>

      <div className="velion-technology__copy">
        <span className="velion-copy-rule fade-out-top" data-fade-out-top aria-hidden="true" />
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          Monitor, brief, approve, act.
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            The useful Velion loop starts with change: a customer message, website update, competitor movement,
            connector gap, or knowledge conflict. Velion turns that signal into a brief, proposes the next action, and
            waits when approval is required.
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
          See the wedge
        </ArrowButton>
      </div>
    </section>
  );
}
