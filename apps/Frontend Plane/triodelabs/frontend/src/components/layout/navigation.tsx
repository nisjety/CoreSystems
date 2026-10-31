"use client";

import {
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent,
  useEffect,
  useRef,
  useState,
} from "react";
import Image from "next/image";
import Link from "next/link";
import { AnimatePresence, motion } from "framer-motion";
import { ArrowRight, Menu, X } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { cn, scrollToElement } from "@/lib/utils";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import type { Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import { LanguageSwitcher } from "./language-switcher";
import { navigationContent } from "./navigation-content";
import {
  navigationDropdowns,
  type NavigationDropdown,
  type NavigationDropdownId,
} from "./navigation-dropdowns";

interface NavigationProps {
  locale: Locale;
  theme?: "dark" | "light";
  variant?: "default" | "editorial";
  surfaceOnScroll?: boolean;
}

type DropdownName = NavigationDropdownId;

const editorialNavigationSurfaceClass = "bg-[#FCFCFA]";

const defaultNavigationSurfaceClass =
  "border-white/40 bg-white/75 backdrop-blur-sm";

function DropdownChevron({ open }: { open: boolean }) {
  return (
    <svg
      aria-hidden="true"
      className={`size-3 fill-none stroke-current stroke-[1.3] transition-transform duration-300 ${open ? "rotate-180" : ""}`}
      viewBox="0 0 12 12"
    >
      <path d="m2 4 4 4 4-4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function NavigationDropdownPanel({
  dropdown,
  locale,
  onClose,
  variant,
}: {
  dropdown: NavigationDropdown;
  locale: Locale;
  onClose: () => void;
  variant: "default" | "editorial";
}) {
  const content = navigationContent[locale];
  const isProducts = dropdown.id === "products";
  const surfaceClass =
    variant === "editorial"
      ? editorialNavigationSurfaceClass
      : defaultNavigationSurfaceClass;

  return (
    <>
      <button
        aria-label={content.closeDropdown(dropdown.title)}
        className="fixed inset-0 z-[55] cursor-default bg-[#0D3E44]/8 backdrop-blur-[2px]"
        onClick={onClose}
        type="button"
      />
      <section
        aria-label={content.dropdownLabel(dropdown.title)}
        className={`fixed inset-x-0 z-[56] max-h-[calc(100svh-72px)] overflow-y-auto border-b ${surfaceClass} p-[clamp(20px,2.8vw,56px)] text-[#161616] shadow-[0_24px_70px_rgba(13,62,68,0.16)] ${
          variant === "editorial"
            ? "top-[92px] max-h-[calc(100svh-92px)] border-[#E1E1DD]/80"
            : "top-[72px]"
        }`}
        id={`navigation-dropdown-${dropdown.id}`}
      >
        <div className="grid gap-8 lg:grid-cols-[minmax(14rem,0.34fr)_minmax(0,1fr)] lg:gap-12">
          <div className="flex min-h-full flex-col items-start">
            <p className="m-0 font-label text-[0.68rem] uppercase tracking-[0.14em] text-[#626260]">
              {dropdown.eyebrow}
            </p>
            <h2 className="mb-0 mt-5 font-manrope text-[clamp(2rem,3.2vw,3.8rem)] font-light leading-[0.88] tracking-[-0.065em]">
              {dropdown.title}.
            </h2>
            <p className="mb-0 mt-5 max-w-[19rem] font-body text-[0.98rem] font-light leading-[1.45] text-[#626260]">
              {dropdown.description}
            </p>
            <Link
              className="mt-8 inline-flex items-center gap-3 rounded-full border border-[#161616]/30 px-4 py-2 font-label text-[0.68rem] uppercase tracking-[0.1em] transition-colors hover:bg-[#161616] hover:text-[#F7F7F4] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
              href={localizeHref(dropdown.overview.href, locale)}
              onClick={onClose}
            >
              {dropdown.overview.label}{" "}
              <ArrowRight aria-hidden="true" className="h-3.5 w-3.5" />
            </Link>
          </div>

          <div
            className={`grid gap-3 ${
              isProducts ? "sm:grid-cols-2" : "sm:grid-cols-2 xl:grid-cols-3"
            }`}
          >
            {dropdown.items.map((item) => (
              <Link
                className="group flex flex-col overflow-hidden rounded-[0.85rem] border border-white/50 bg-white/40 backdrop-blur-md transition-colors hover:bg-white/60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[#161616]"
                href={localizeHref(item.href, locale)}
                key={item.href}
                onClick={onClose}
                rel={item.external ? "noreferrer" : undefined}
                target={item.external ? "_blank" : undefined}
              >
                {item.image && (
                  <div className="relative aspect-[4/3] w-full overflow-hidden">
                    <Image
                      alt=""
                      className="object-cover transition-transform duration-500 group-hover:scale-105"
                      fill
                      sizes={
                        isProducts
                          ? "(min-width: 640px) 40vw, 100vw"
                          : "(min-width: 1280px) 28vw, (min-width: 640px) 50vw, 100vw"
                      }
                      src={item.image}
                      style={
                        item.imagePosition
                          ? { objectPosition: item.imagePosition }
                          : undefined
                      }
                    />
                  </div>
                )}
                <div className="flex flex-1 flex-col gap-6 p-4">
                  <div className="flex items-start justify-between gap-4 font-label text-[0.66rem] uppercase tracking-[0.12em] text-[#626260] transition-colors group-hover:text-[#161616]">
                    <span>{item.number}</span>
                    <span aria-hidden="true">
                      <ArrowRight className="h-3.5 w-3.5" />
                    </span>
                  </div>
                  <div>
                    <h3 className="m-0 font-manrope text-[clamp(1.15rem,1.5vw,1.6rem)] font-light leading-[0.98] tracking-[-0.045em]">
                      {item.label}
                    </h3>
                    <p className="mb-0 mt-2 font-body text-[0.82rem] font-light leading-[1.3] text-[#626260] transition-colors group-hover:text-[#161616]">
                      {item.description}
                    </p>
                  </div>
                </div>
              </Link>
            ))}
          </div>
        </div>
      </section>
    </>
  );
}

export function Navigation({
  locale,
  theme = "light",
  variant = "default",
  surfaceOnScroll = true,
}: NavigationProps) {
  const content = navigationContent[locale];
  const dropdowns = navigationDropdowns[locale];
  const [isScrolled, setIsScrolled] = useState(false);
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const [menuKeyboardAction, setMenuKeyboardAction] = useState(false);
  const [activeDropdown, setActiveDropdown] = useState<DropdownName | null>(
    null,
  );
  const isEditorial = variant === "editorial";

  useEffect(() => {
    let frame: number | null = null;

    const update = () => {
      const threshold = isEditorial ? 24 : 20;
      setIsScrolled((current) => {
        const next = window.scrollY > threshold;
        return current === next ? current : next;
      });
      frame = null;
    };

    const scheduleUpdate = () => {
      if (frame === null) {
        frame = window.requestAnimationFrame(update);
      }
    };

    update();
    window.addEventListener("scroll", scheduleUpdate, { passive: true });
    window.addEventListener("resize", scheduleUpdate);

    return () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
      window.removeEventListener("scroll", scheduleUpdate);
      window.removeEventListener("resize", scheduleUpdate);
    };
  }, [isEditorial]);

  useEffect(() => {
    if (!isMenuOpen) {
      return;
    }

    const previousOverflow = document.body.style.overflow;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMenuKeyboardAction(true);
        setIsMenuOpen(false);
      }
    };

    document.body.style.overflow = "hidden";
    window.addEventListener("keydown", closeOnEscape);

    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [isMenuOpen]);

  useEffect(() => {
    if (activeDropdown === null) {
      return;
    }

    const previousOverflow = document.body.style.overflow;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setActiveDropdown(null);
      }
    };

    document.body.style.overflow = "hidden";
    window.addEventListener("keydown", closeOnEscape);

    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [activeDropdown]);

  const handleNavClick = (href: string) => {
    if (href.startsWith("/")) {
      window.location.href = localizeHref(href, locale);
    } else {
      scrollToElement(href.replace("#", ""));
    }
    setIsMenuOpen(false);
  };

  if (isEditorial) {
    return (
      <EditorialNavigation
        locale={locale}
        isMenuOpen={isMenuOpen}
        isScrolled={isScrolled}
        surfaceOnScroll={surfaceOnScroll}
        activeDropdown={activeDropdown}
        onClose={() => setIsMenuOpen(false)}
        onDropdownChange={setActiveDropdown}
        onOpen={(keyboard) => {
          setMenuKeyboardAction(keyboard);
          setIsMenuOpen(true);
        }}
        menuKeyboardAction={menuKeyboardAction}
        theme={theme}
      />
    );
  }

  return (
    <>
      <motion.nav
        className={cn(
          "fixed left-0 right-0 top-0 z-50 transition-all duration-500 ease-out",
          isScrolled ? "bg-white/75 shadow-md backdrop-blur-sm" : "",
        )}
        initial={{ y: -100 }}
        animate={{ y: 0 }}
        transition={{ duration: 0.8, ease: [0.25, 1, 0.5, 1] }}
      >
        <div className="section-shell">
          <div className="flex h-16 items-center justify-between">
            <button
              type="button"
              aria-label={content.homeButton}
              className="mr-auto cursor-pointer"
              onClick={() => (window.location.href = localizeHref("/", locale))}
            >
              <span className="relative font-label text-xl font-light tracking-wide text-black">
                TRIODELAB
                <span
                  aria-hidden="true"
                  className="absolute left-[calc(50%+0.5rem)] top-0 h-1.5 w-1.5 -translate-x-1/2 -translate-y-0.5 bg-black"
                />
                <span
                  aria-hidden="true"
                  className="absolute bottom-0 left-[calc(50%-1.5rem)] h-0.5 w-12 -translate-x-1/2 translate-y-0.5 bg-black"
                />
              </span>
            </button>

            <div className="ml-auto hidden items-center space-x-6 md:flex">
              {content.links.map((item) => {
                if (item.dropdown) {
                  const dropdownName = item.dropdown;
                  const isOpen = activeDropdown === dropdownName;

                  return (
                    <button
                      aria-controls={`navigation-dropdown-${dropdownName}`}
                      aria-expanded={isOpen}
                      className="inline-flex items-center gap-1.5 font-label text-sm font-light text-black transition-colors duration-200 hover:text-slate-600"
                      key={item.name}
                      onClick={() =>
                        setActiveDropdown(isOpen ? null : dropdownName)
                      }
                      type="button"
                    >
                      {item.name}
                      <DropdownChevron open={isOpen} />
                    </button>
                  );
                }

                return (
                  <button
                    key={item.name}
                    type="button"
                    onClick={() => handleNavClick(item.href)}
                    className="font-label text-sm font-light text-black transition-colors duration-200 hover:text-slate-600"
                  >
                    {item.name}
                  </button>
                );
              })}
              <Button
                onClick={() => scrollToElement("kontakt")}
                className="ml-6 border-none bg-black px-4 py-2 font-label text-sm font-light text-white transition-all duration-300 hover:bg-slate-800"
              >
                {content.getStarted}
              </Button>
              <LanguageSwitcher className="text-black" locale={locale} />
            </div>

            <button
              type="button"
              className="ml-auto p-2 text-black transition-colors hover:text-slate-600 md:hidden"
              onClick={() => setIsMenuOpen(!isMenuOpen)}
              aria-expanded={isMenuOpen}
              aria-label={isMenuOpen ? content.closeMenu : content.openMenu}
            >
              {isMenuOpen ? <X size={20} /> : <Menu size={20} />}
            </button>
          </div>

          {isMenuOpen && (
            <div className="border-t border-slate-200 bg-white py-4 md:hidden">
              <LanguageSwitcher className="px-4 pb-2 text-black" locale={locale} />
              {content.links.map((item) => {
                const dropdown = item.dropdown ? dropdowns[item.dropdown] : null;

                return (
                  <div key={item.name}>
                    <button
                      type="button"
                      onClick={() => handleNavClick(item.href)}
                      className="block w-full px-4 py-2 text-left font-label text-sm font-light text-black hover:bg-slate-50"
                    >
                      {item.name}
                    </button>
                    {dropdown && (
                      <div className="mb-2 ml-4 border-l border-slate-200 py-1">
                        {dropdown.items.map((subItem) => (
                          <a
                            className="block px-4 py-1.5 font-label text-xs font-light text-slate-600 hover:text-black"
                            href={localizeHref(subItem.href, locale)}
                            key={subItem.href}
                            onClick={() => setIsMenuOpen(false)}
                            rel={subItem.external ? "noreferrer" : undefined}
                            target={subItem.external ? "_blank" : undefined}
                          >
                            {subItem.label}
                          </a>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </motion.nav>
      {activeDropdown && (
        <NavigationDropdownPanel
          dropdown={dropdowns[activeDropdown]}
          locale={locale}
          onClose={() => setActiveDropdown(null)}
          variant="default"
        />
      )}
    </>
  );
}

interface EditorialNavigationProps {
  locale: Locale;
  activeDropdown: DropdownName | null;
  isMenuOpen: boolean;
  isScrolled: boolean;
  menuKeyboardAction: boolean;
  onClose: () => void;
  onDropdownChange: (dropdown: DropdownName | null) => void;
  onOpen: (keyboard: boolean) => void;
  surfaceOnScroll: boolean;
  theme: "dark" | "light";
}

function EditorialNavigation({
  locale,
  activeDropdown,
  isMenuOpen,
  isScrolled,
  menuKeyboardAction,
  onClose,
  onDropdownChange,
  onOpen,
  surfaceOnScroll,
  theme,
}: EditorialNavigationProps) {
  const content = navigationContent[locale];
  const dropdowns = navigationDropdowns[locale];
  const prefersReducedMotion = usePrefersReducedMotion();
  const hasNavigationSurface =
    (surfaceOnScroll && isScrolled) || activeDropdown !== null;
  const isLightText = !hasNavigationSurface && theme === "dark";
  const menuRef = useRef<HTMLDivElement>(null);
  const menuTriggerRef = useRef<HTMLButtonElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const wasMenuOpen = useRef(false);

  useEffect(() => {
    if (isMenuOpen) {
      wasMenuOpen.current = true;
      closeButtonRef.current?.focus();
      return;
    }

    if (wasMenuOpen.current) {
      menuTriggerRef.current?.focus();
      wasMenuOpen.current = false;
    }
  }, [isMenuOpen]);

  const handleAnchorClick = (
    event: MouseEvent<HTMLAnchorElement>,
    href: string,
  ) => {
    if (!href.startsWith("#")) {
      onClose();
      return;
    }

    event.preventDefault();
    scrollToElement(href.replace("#", ""));
    onClose();
  };

  const handleMenuKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab" || !menuRef.current) {
      return;
    }

    const focusableElements = Array.from(
      menuRef.current.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    ).filter((element) => element.getClientRects().length > 0);

    if (focusableElements.length === 0) {
      return;
    }

    const firstElement = focusableElements[0];
    const lastElement = focusableElements[focusableElements.length - 1];
    const activeElement = document.activeElement;

    if (event.shiftKey && activeElement === firstElement) {
      event.preventDefault();
      lastElement.focus();
    } else if (!event.shiftKey && activeElement === lastElement) {
      event.preventDefault();
      firstElement.focus();
    }
  };

  return (
    <>
      <header
        className={[
          "pointer-events-none fixed inset-x-0 top-0 z-[60] grid grid-cols-[minmax(160px,1fr)_auto_minmax(160px,1fr)] items-center px-[clamp(24px,4vw,56px)] transition-[height,background-color,backdrop-filter,box-shadow,color] duration-300 max-[1023px]:h-24 max-[1023px]:grid-cols-[1fr_auto]",
          isLightText
            ? "text-white [text-shadow:0_1px_10px_rgba(0,0,0,0.25)]"
            : "text-[#161616]",
          hasNavigationSurface
            ? `h-[92px] ${editorialNavigationSurfaceClass}`
            : "h-28 border-b border-transparent bg-transparent",
        ].join(" ")}
        data-site-nav=""
      >
        <Link
          aria-label={content.home}
          className="pointer-events-auto justify-self-start font-label text-[0.98rem] font-light uppercase leading-none tracking-[0.18em] max-[760px]:text-[0.86rem] max-[760px]:tracking-[0.16em]"
          href={localizeHref("/", locale)}
        >
          TRIODELAB
        </Link>

        <nav
          aria-label={content.mainNavigation}
          className="pointer-events-auto flex items-center gap-[clamp(42px,5.25vw,90px)] justify-self-center max-[1023px]:hidden"
        >
          {content.links.map((item) => {
            if (item.dropdown) {
              const dropdownName = item.dropdown;
              const isOpen = activeDropdown === dropdownName;

              return (
                <button
                  aria-controls={`navigation-dropdown-${dropdownName}`}
                  aria-expanded={isOpen}
                  className="relative inline-flex items-center justify-center gap-1.5 py-[41px] pb-7 font-label text-[0.78rem] font-light leading-none tracking-[0.03em] after:absolute after:bottom-[17px] after:left-1/2 after:h-px after:w-[34px] after:-translate-x-1/2 after:scale-x-50 after:bg-current after:opacity-0 after:transition after:content-[''] hover:after:scale-x-100 hover:after:opacity-80"
                  key={item.href}
                  onClick={() => onDropdownChange(isOpen ? null : dropdownName)}
                  type="button"
                >
                  {item.name}
                  <DropdownChevron open={isOpen} />
                </button>
              );
            }

            return item.href.startsWith("/") ? (
              <Link
                className="relative inline-flex items-center justify-center py-[41px] pb-7 font-label text-[0.78rem] font-light leading-none tracking-[0.03em] after:absolute after:bottom-[17px] after:left-1/2 after:h-px after:w-[34px] after:-translate-x-1/2 after:scale-x-50 after:bg-current after:opacity-0 after:transition after:content-[''] hover:after:scale-x-100 hover:after:opacity-80"
                href={localizeHref(item.href, locale)}
                key={item.href}
              >
                {item.name}
              </Link>
            ) : (
              <a
                className="relative inline-flex items-center justify-center py-[41px] pb-7 font-label text-[0.78rem] font-light leading-none tracking-[0.03em] after:absolute after:bottom-[17px] after:left-1/2 after:h-px after:w-[34px] after:-translate-x-1/2 after:scale-x-50 after:bg-current after:opacity-0 after:transition after:content-[''] hover:after:scale-x-100 hover:after:opacity-80"
                href={item.href}
                key={item.href}
                onClick={(event) => handleAnchorClick(event, item.href)}
              >
                {item.name}
              </a>
            );
          })}
        </nav>

        <div className="pointer-events-auto flex items-center gap-[30px] justify-self-end max-[760px]:gap-[18px]">
          <LanguageSwitcher locale={locale} />
          <Link
            aria-label={content.contact}
            className={[
              "grid size-[34px] place-items-center transition-colors max-[760px]:size-8",
              isLightText
                ? "text-white/75 hover:text-white"
                : "text-[#161616]/70 hover:text-[#161616]",
            ].join(" ")}
            href={content.contactEmailHref}
          >
            <svg
              aria-hidden="true"
              className="size-[18px] overflow-visible fill-none stroke-current stroke-[1px] max-[760px]:size-[17px]"
              viewBox="0 0 20 20"
            >
              <rect x="2.5" y="4.25" width="15" height="11.5" rx="1" />
              <path
                d="m3.25 5 6.75 5 6.75-5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </Link>

          <button
            aria-expanded={isMenuOpen}
            aria-label={isMenuOpen ? content.closeMenu : content.openMenu}
            className={[
              "group grid w-[50px] cursor-pointer gap-1.5 border-0 bg-transparent py-2.5 transition-colors max-[760px]:w-11",
              isLightText
                ? "text-white/85 hover:text-white"
                : "text-[#161616]/80 hover:text-[#161616]",
            ].join(" ")}
            onClick={(event) => onOpen(event.detail === 0)}
            ref={menuTriggerRef}
            type="button"
          >
            <span className="block h-px w-full origin-right bg-current transition-transform group-hover:scale-x-[0.76]" />
            <span className="block h-px w-full origin-right bg-current transition-transform group-hover:scale-x-90" />
          </button>
        </div>
      </header>

      {activeDropdown && (
        <NavigationDropdownPanel
          dropdown={dropdowns[activeDropdown]}
          locale={locale}
          onClose={() => onDropdownChange(null)}
          variant="editorial"
        />
      )}

      <AnimatePresence>
        {isMenuOpen && (
          <motion.div
            initial={
              menuKeyboardAction || prefersReducedMotion
                ? false
                : { opacity: 0, y: -8 }
            }
            animate={{ opacity: 1, y: 0 }}
            exit={
              menuKeyboardAction || prefersReducedMotion
                ? { opacity: 0, transition: { duration: 0 } }
                : { opacity: 0, y: -8, transition: { duration: 0.15 } }
            }
            transition={{ duration: 0.18, ease: [0.25, 1, 0.5, 1] }}
            aria-label={content.menu}
            aria-modal="true"
            className="triodelab-menu fixed inset-0 z-[100] grid grid-rows-[auto_1fr_auto] overflow-y-auto overscroll-contain bg-[linear-gradient(115deg,rgba(145,189,202,0.2),transparent_52%),linear-gradient(135deg,#153845,#0d2630_68%,#071820)] px-[clamp(24px,4vw,56px)] pb-14 pt-[54px] text-[#fafbfb] max-[760px]:grid-rows-[auto_auto_auto] max-[760px]:gap-10 max-[760px]:pb-[34px] max-[760px]:pt-[34px]"
            onKeyDown={handleMenuKeyDown}
            ref={menuRef}
            role="dialog"
          >
            <div className="flex items-start justify-between">
              <Link
                aria-label={content.home}
                className="font-label text-[0.98rem] font-light uppercase leading-none tracking-[0.18em]"
                href={localizeHref("/", locale)}
                onClick={onClose}
              >
                TRIODELAB
              </Link>
              <button
                aria-label={content.closeMenu}
                className="grid h-[58px] w-[58px] cursor-pointer place-items-center border-0 bg-transparent p-0 text-current transition-opacity hover:opacity-70"
                onClick={onClose}
                ref={closeButtonRef}
                type="button"
              >
                <svg
                  aria-hidden="true"
                  className="size-full fill-none stroke-current stroke-[1.5]"
                  viewBox="0 0 40 40"
                >
                  <path d="M6 6l28 28M34 6 6 34" />
                </svg>
              </button>
            </div>

            <nav
              aria-label={content.mainMenu}
              className="grid grid-cols-4 gap-7 self-center max-[1100px]:grid-cols-2 max-[760px]:grid-cols-1 max-[760px]:gap-[22px] max-[760px]:self-start"
            >
              {content.links.map((item, index) => {
                const dropdown = item.dropdown ? dropdowns[item.dropdown] : null;

                return (
                  <div
                    className="grid gap-4 border-t border-white/20 pt-2"
                    key={item.href}
                  >
                    <Link
                      className="grid gap-[38px] transition-colors hover:text-white max-[760px]:gap-[18px]"
                      href={localizeHref(item.href, locale)}
                      onClick={onClose}
                    >
                      <span className="font-label text-[0.98rem] font-light text-white/72">
                        {String(index + 1).padStart(3, "0")}
                      </span>
                      <strong className="font-label text-[2.2rem] font-light leading-none text-white/80 max-[760px]:text-[1.72rem]">
                        {item.name}
                      </strong>
                    </Link>
                    {dropdown && (
                      <div className="grid gap-2 border-l border-white/20 pl-4">
                        {dropdown.items.map((subItem) => (
                          <a
                            className="inline-flex items-center gap-1.5 font-label text-[0.94rem] font-normal text-white/85 transition-colors hover:text-white"
                            href={localizeHref(subItem.href, locale)}
                            key={subItem.href}
                            onClick={onClose}
                            rel={subItem.external ? "noreferrer" : undefined}
                            target={subItem.external ? "_blank" : undefined}
                          >
                            {subItem.label}{" "}
                            <ArrowRight
                              aria-hidden="true"
                              className="h-3.5 w-3.5"
                            />
                          </a>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </nav>

            <div className="flex flex-wrap items-end justify-between gap-6">
              <div className="grid justify-items-start gap-2 font-label text-base font-light text-white/74">
                <span>{content.menuFooter}</span>
                <Link
                  className="transition-colors hover:text-white"
                  href={localizeHref("/kontakt", locale)}
                  onClick={onClose}
                >
                  {content.startProject}
                </Link>
              </div>
              <LanguageSwitcher className="text-sm" locale={locale} />
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}
