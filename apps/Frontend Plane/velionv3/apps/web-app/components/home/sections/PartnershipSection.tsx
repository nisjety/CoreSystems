import { ArrowButton } from "@/components/ui/buttons/ArrowButton";
import { VisualPanel } from "./VisualPanel";

export function PartnershipSection() {
  return (
    <section className="velion-section velion-section--partnership" id="partnership">
      <div className="velion-section__copy">
        <span className="velion-copy-rule fade-out-top" data-fade-out-top aria-hidden="true" />
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          Joint Forces: BMW and VELION
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            Spearheading electrified mobility, the collaboration of BMW and VELION gave birth to a groundbreaking yacht.
            Unprecedented in its design, technology, and purpose. Poised to revolutionize the luxury marine sector.
          </p>
          <p>
            Initiating the project, BMW leveraged their technology and innovation leadership role in e-mobility to spark
            the creation of a groundbreaking vessel. BMW contributed their visionary design, automotive know-how and
            advanced technical components to demonstrate, how sustainable future-oriented mobility on the water can be
            realized today. The partnership with VELION brought the concept to life: a perfect blend of luxury,
            sustainability, and innovation where the road meets the waves.
          </p>
        </div>
        <div className="velion-copy-action fade-out-top" data-fade-out-top>
          <ArrowButton href="#technology" variant="muted">
            To dive deeper, please click here:
          </ArrowButton>
        </div>
      </div>
      <VisualPanel className="velion-section__visual" label="Velion partnership study" variant="icon" />
    </section>
  );
}
