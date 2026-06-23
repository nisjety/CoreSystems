import { ArrowButton } from "@/components/ui/buttons/ArrowButton";
import { CpuArchitecture } from "@/components/ui/cpu-architecture";

const stats = [
  { label: "AI worker loop", unit: "steps", value: "4" },
  { label: "Manual parity", unit: "%", value: "100" },
  { label: "Risky actions", unit: "approved", value: "HITL" },
];

export function TechnologySection() {
  return (
    <section className="velion-technology" id="technology">
      <div className="velion-technology__visual">
        <div className="velion-technology__diagram-shell">
          <div style={{ display: "inline-block", transform: "scale(0.65)", transformOrigin: "center" }}>
            <CpuArchitecture />
          </div>        
        </div>
      </div>

      <div className="velion-technology__copy">
        <span className="velion-copy-rule" aria-hidden="true" />
        <h1 className="header-1">
          Monitor, brief, approve, act.
        </h1>
        <div className="velion-ingress ingress">
          <p>
            The useful Velion loop starts with change: a customer message, website update, competitor movement,
            connector gap, or knowledge conflict. Velion turns that signal into a brief, proposes the next action, and
            waits when approval is required.
          </p>
        </div>
        <div className="relative z-10 top-90">
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
      </div>
    </section>
  );
}
