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
          History in the making
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            Embark on a journey through our roots and delve into VELION&apos;s dynamic story. Discover the individuals and
            partners behind our shared vision and the remarkable launch of The Icon.
          </p>
        </div>
        <div className="velion-copy-action fade-out-top" data-fade-out-top>
          <ArrowButton href="#contact" variant="dark">
            Contact
          </ArrowButton>
        </div>
      </div>
      <VisualPanel className="velion-section__visual" label="Electric yacht wake at dusk" variant="heritage" />
    </section>
  );
}
