import type { CSSProperties } from "react";

const loopStates = ["Ask", "Draft", "Approve", "Audit"];

export function ProductLoopSection() {
  return (
    <section className="velion-product-loop" data-product-loop id="product-loop">
      <div className="velion-product-loop__copy">
        <a className="velion-product-loop__eyebrow fade-out-top" data-fade-out-top href="#technology">
          Early access: customer experience system
          <span aria-hidden="true">-&gt;</span>
        </a>
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          Run support work on autopilot.
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            Ask in natural language. Velion can draft replies, build workflows, pause for approval, and keep the source
            trail visible before any action reaches a customer.
          </p>
        </div>
        <a className="velion-product-loop__cta fade-out-top" data-fade-out-top href="#technology">
          Follow the work loop
          <span aria-hidden="true">-&gt;</span>
        </a>
      </div>

      <div className="velion-product-loop__stage" aria-label="Animated Velion dashboard screenshots">
        <div className="velion-product-loop__dashboard">
          <div className="velion-product-loop__video-frame">
            <video
              aria-label="Velion dashboard recording with Norwegian prompts typed into the composer."
              autoPlay
              loop
              muted
              playsInline
              poster="/velion-product-shots/dashboard-expanded-prompt.png"
              preload="metadata"
            >
              <source src="/velion-product-shots/velion-dashboard-typing.mp4" type="video/mp4" />
            </video>
          </div>

          <div className="velion-product-loop__state-row" aria-label="Velion work loop states">
            {loopStates.map((state, index) => (
              <div
                className="velion-product-loop__state"
                key={state}
                style={{ "--state-index": index } as CSSProperties}
              >
                <span>{state}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
