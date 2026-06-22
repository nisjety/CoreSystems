type NavbarProps = {
  isMenuOpen: boolean;
  isOnDark: boolean;
  isScrolled: boolean;
  onOpen: () => void;
};

const navItems = [
  { href: "#partnership", label: "Product" },
  { href: "#trust", label: "Trust" },
  { href: "#workflows", label: "Workflows" },
  { href: "#product-loop", label: "Demo" },
];

export function Navbar({ isMenuOpen, isOnDark, isScrolled, onOpen }: NavbarProps) {
  return (
    <header
      className={`velion-navbar ${isScrolled ? "is-scrolled" : ""} ${isOnDark ? "is-on-dark" : ""}`}
    >
      <a aria-label="Velion home" className="velion-navbar__brand" href="#top">
        VELION
      </a>

      <nav aria-label="Primary navigation" className="velion-navbar__links">
        {navItems.map((item, index) => (
          <a
            className={`velion-navbar__link ${index === 0 ? "is-active" : ""}`}
            href={item.href}
            key={item.href}
          >
            {item.label}
          </a>
        ))}
      </nav>

      <div className="velion-navbar__actions">
        <a aria-label="Search" className="velion-navbar__search" href="#contact">
          <svg aria-hidden="true" viewBox="0 0 20 20">
            <circle cx="8.5" cy="8.5" r="5.75" />
            <path d="m13 13 4 4" />
          </svg>
        </a>

        <button
          aria-expanded={isMenuOpen}
          aria-label="Open menu"
          className="velion-navbar__menu"
          onClick={onOpen}
          type="button"
        >
          <span />
          <span />
          <span />
        </button>
      </div>
    </header>
  );
}
