type NavbarProps = {
  isMenuOpen: boolean;
  isScrolled: boolean;
  onOpen: () => void;
};

export function Navbar({ isMenuOpen, isScrolled, onOpen }: NavbarProps) {
  return (
    <header className={`velion-navbar ${isScrolled ? "is-scrolled" : ""}`}>
      <a aria-label="Velion home" className="velion-navbar__brand" href="#top">
        VELION
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
    </header>
  );
}
