const siteLinks = [
  { href: "#top", label: "Home" },
  { href: "#partnership", label: "Product" },
  { href: "#technology", label: "Work loop" },
  { href: "#company", label: "Trust" },
  { href: "#contact", label: "Access" },
];

const workflowLinks = ["Replacing Tab-Heavy Workflows", "Knowledge graph inspection", "Human approval gates"];
const legalLinks = ["Privacy Policy", "Imprint", "Security", "Audit History"];

export function Footer() {
  return (
    <footer className="velion-footer" id="contact" data-footer-parallax>
      <div className="velion-footer__media" aria-hidden="true" data-footer-parallax-media />

      <div className="velion-footer__inner" data-footer-parallax-content data-footer-parallax-inner>
        <div className="velion-footer__masthead">
          <div className="velion-footer__brand" aria-label="Velion" data-footer-parallax-brand>
            VELION
          </div>

          <div className="velion-footer__statement">
            <p className="velion-footer__kicker">Replacing Tab-Heavy Workflows</p>
            <h2>Stop losing customers to downtime.</h2>
            <p>
              Velion keeps support, sales assistance, knowledge, and workflow execution moving for
              security-first teams, with approvals and audit history built into every action.
            </p>
          </div>

          <div className="velion-footer__newsletter">
            <p>Request early access</p>
            <div className="velion-footer__input">
              <input aria-label="Work email address" placeholder="Work e-mail address" type="email" />
              <button aria-label="Submit newsletter" type="button">
                <svg aria-hidden="true" viewBox="0 0 24 12">
                  <path d="M1 6h20M16 1l5 5-5 5" />
                </svg>
              </button>
            </div>
            <small>Built for teams that need source-backed automation, approval boundaries, and rollback.</small>
          </div>
        </div>

        <div className="velion-footer__bottom">
          <nav aria-label="Site index" className="velion-footer__group">
            <h3>Site Index</h3>
            {siteLinks.map((link) => (
              <a href={link.href} key={link.label}>
                {link.label}
              </a>
            ))}
          </nav>

          <div className="velion-footer__group" aria-label="Workflow themes">
            <h3>Workflows</h3>
            {workflowLinks.map((link) => (
              <span key={link}>{link}</span>
            ))}
          </div>

          <div className="velion-footer__group">
            <h3>Get in touch</h3>
            <a href="mailto:hello@velion.ai">hello@velion.ai</a>
            <span>Oslo / Remote</span>
          </div>

          <nav aria-label="Legal" className="velion-footer__group">
            <h3>Legal</h3>
            {legalLinks.map((link) => (
              <a href="#top" key={link}>
                {link}
              </a>
            ))}
          </nav>
        </div>
      </div>
    </footer>
  );
}
