const footerLinks = [
  "Yachts",
  "THE ICON",
  "THE OPEN",
  "Technology",
  "Company",
  "Contact",
];

const legalLinks = ["Privacy Policy", "Imprint", "Instagram", "LinkedIn", "YouTube"];

export function Footer() {
  return (
    <footer className="velion-footer" id="contact">
      <div className="velion-footer__inner">
        <div className="velion-footer__brand" aria-label="Velion">
          VELION
        </div>

        <nav aria-label="Footer navigation" className="velion-footer__nav">
          {footerLinks.map((link, index) => (
            <a href={link === "Technology" ? "#technology" : "#top"} key={link}>
              <span>{String(index + 1).padStart(3, "0")}</span>
              {link}
            </a>
          ))}
        </nav>

        <div className="velion-footer__newsletter">
          <p>Sign up to our newsletter</p>
          <div className="velion-footer__input">
            <input aria-label="Email address" placeholder="Your e-mail address" type="email" />
            <button aria-label="Submit newsletter" type="button">
              <svg aria-hidden="true" viewBox="0 0 24 12">
                <path d="M1 6h20M16 1l5 5-5 5" />
              </svg>
            </button>
          </div>
          <small>
            By submitting this form, you agree to our <a href="#top">Privacy Policy</a>
          </small>
        </div>

        <div className="velion-footer__legal">
          {legalLinks.map((link) => (
            <a href="#top" key={link}>
              {link}
            </a>
          ))}
        </div>
      </div>
    </footer>
  );
}
