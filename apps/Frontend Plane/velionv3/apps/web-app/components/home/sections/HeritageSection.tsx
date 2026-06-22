import { ArrowButton } from "@/components/ui/buttons/ArrowButton";
import { VisualPanel } from "./VisualPanel";

export function HeritageSection() {
  return (
    <section className="velion-section velion-section--split velion-section--last" id="company">
      <div className="velion-section__copy">
        <span className="velion-copy-label fade-out-top" data-fade-out-top>
          003
        </span>
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          Built for grounded Norwegian work.
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            Velion is designed for companies that need real customer work, real source grounding, and real control. The
            wedge is local intelligence: Brreg-grounded company context, in-region operations, and an auditable path from
            signal to approved action.
          </p>
        </div>
        <div className="velion-copy-action fade-out-top" data-fade-out-top>
          <ArrowButton href="#contact" variant="dark">
            Request access
          </ArrowButton>
        </div>
      </div>
      <VisualPanel className="velion-section__visual" label="Norwegian intelligence and approved action map" variant="heritage" />
    </section>
  );
}
