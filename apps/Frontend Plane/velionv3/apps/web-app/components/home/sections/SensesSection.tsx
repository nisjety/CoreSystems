import { VisualPanel } from "./VisualPanel";

export function SensesSection() {
  return (
    <section className="velion-section velion-section--reverse velion-section--senses">
      <VisualPanel className="velion-section__visual" label="Velion source graph and knowledge trace" variant="open" />
      <div className="velion-section__copy">
        <span className="velion-copy-rule fade-out-top" data-fade-out-top aria-hidden="true" />
        <h1 className="header-1 fade-out-top" data-fade-out-top>
          It explains what it learned.
        </h1>
        <div className="velion-ingress ingress fade-out-top" data-fade-out-top>
          <p>
            Velion can connect websites, documents, inbox history, integrations, and company registers. It does not only
            ingest them. It builds a working memory that humans can inspect, correct, and use.
          </p>
          <p>
            When the worker drafts a reply or proposes a workflow, the interface keeps the source trail visible:
            retrieved passages, policies, connector state, confidence, and the action it wants permission to take.
          </p>
          <p>Knowledge is not hidden behind the model. It becomes an operating surface.</p>
        </div>
      </div>
    </section>
  );
}
