type MenuModalProps = {
  onClose: () => void;
  open: boolean;
};

const menuItems = [
  { href: "#product-loop", label: "Demo", number: "001" },
  { href: "#workflows", label: "Workflows", number: "002" },
  { href: "#trust", label: "Trust", number: "003" },
  { href: "#contact", label: "Access", number: "004" },
];

const socialLinks = ["Instagram", "LinkedIn", "YouTube"];

export function MenuModal({ onClose, open }: MenuModalProps) {
  if (!open) {
    return null;
  }

  return (
    <div aria-label="Velion menu" aria-modal="true" className="velion-menu" role="dialog">
      <div className="velion-menu__top">
        <a aria-label="Velion home" className="velion-menu__mark" href="#top" onClick={onClose}>
          V
        </a>
        <button aria-label="Close menu" className="velion-menu__close" onClick={onClose} type="button">
          <svg aria-hidden="true" viewBox="0 0 40 40">
            <path d="M6 6l28 28M34 6 6 34" />
          </svg>
        </button>
      </div>

      <nav aria-label="Primary menu" className="velion-menu__grid">
        {menuItems.map((item, index) => (
          <a
            className="velion-menu__item"
            href={item.href}
            key={item.href}
            onClick={onClose}
            style={{ "--delay": `${index * 80}ms` } as React.CSSProperties}
          >
            <span>{item.number}</span>
            <strong>{item.label}</strong>
          </a>
        ))}
      </nav>

      <div className="velion-menu__social" aria-label="Social links">
        <span>Follow us:</span>
        {socialLinks.map((link) => (
          <a href="#contact" key={link} onClick={onClose}>
            {link}
          </a>
        ))}
      </div>
    </div>
  );
}
