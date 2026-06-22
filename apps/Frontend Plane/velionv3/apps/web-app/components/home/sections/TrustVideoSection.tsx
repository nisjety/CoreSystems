import { ArrowButton } from "@/components/ui/buttons/ArrowButton";

export function TrustVideoSection() {
  return (
    <section className="velion-trust-video" id="trust">
      <div className="velion-trust-video__media" aria-label="Velion trust film placeholder" role="img">
        <div className="velion-trust-video__play" aria-hidden="true">
          <svg viewBox="0 0 22 22">
            <path d="M8 5.5v11l8.5-5.5L8 5.5Z" />
          </svg>
        </div>
      </div>

      <div className="velion-trust-video__copy">
        <span className="velion-copy-label fade-out-top" data-fade-out-top>
          Trust signal
        </span>
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          Built locally for controlled AI work.
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            Velion is for teams that need AI to do real customer work with sources, approvals, audit history, and humans
            still in control.
          </p>
        </div>
        <ArrowButton href="#contact" variant="muted">
          Meet Velion
        </ArrowButton>
      </div>
    </section>
  );
}
