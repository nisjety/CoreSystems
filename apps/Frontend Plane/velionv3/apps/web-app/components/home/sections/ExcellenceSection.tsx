export function ExcellenceSection() {
  return (
    <section className="velion-excellence">
      <div className="velion-excellence__media" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
      <div className="velion-excellence__content velion-reveal">
        <h2 className="header-1">Trust is not a badge. It is the interface.</h2>
        <p className="ingress">
          Source traces, approvals, role boundaries, audit events, and rollback paths are treated as first-class product
          surfaces. Velion can move faster because humans can see exactly what it is doing.
        </p>
        <div className="velion-award" aria-label="Human approval mark" role="img">
          <span>Human</span>
          <strong>Approval before risk</strong>
        </div>
      </div>
    </section>
  );
}
