import type { Localized } from "../../lib/i18n/locales";
import type { NavigationDropdownId } from "./navigation-dropdowns";

export type NavigationLink = {
  dropdown?: NavigationDropdownId;
  /** Norwegian path; localized when rendered. */
  href: string;
  name: string;
};

export const navigationContent: Localized<{
  closeDropdown: (title: string) => string;
  closeMenu: string;
  contact: string;
  contactEmailHref: string;
  dropdownLabel: (title: string) => string;
  getStarted: string;
  home: string;
  homeButton: string;
  links: readonly NavigationLink[];
  mainMenu: string;
  mainNavigation: string;
  menu: string;
  menuFooter: string;
  openMenu: string;
  startProject: string;
}> = {
  nb: {
    links: [
      { name: "Tjenester", href: "/tjenester", dropdown: "services" },
      { name: "Rådgivning", href: "/tjenester/radgivning-og-konsulentbistand" },
      { name: "Produkter", href: "/plattform", dropdown: "products" },
      { name: "Om oss", href: "/om-oss" },
      { name: "Kontakt", href: "/kontakt" },
    ],
    home: "TriodeLab hjem",
    homeButton: "Gå til forsiden",
    mainNavigation: "Hovednavigasjon",
    mainMenu: "Hovedmeny",
    menu: "TriodeLab meny",
    openMenu: "Åpne meny",
    closeMenu: "Lukk meny",
    closeDropdown: (title) => `Lukk ${title}-meny`,
    dropdownLabel: (title) => `${title}-meny`,
    contact: "Kontakt",
    contactEmailHref:
      "mailto:post@triodelab.no?subject=Foresp%C3%B8rsel%20fra%20triodelab.no&body=Hei%20TriodeLab%2C%0A%0A",
    getStarted: "Kom i gang",
    menuFooter: "TriodeLab — Oslo",
    startProject: "Start et prosjekt",
  },
  en: {
    links: [
      { name: "Services", href: "/tjenester", dropdown: "services" },
      { name: "Advisory", href: "/tjenester/radgivning-og-konsulentbistand" },
      { name: "Products", href: "/plattform", dropdown: "products" },
      { name: "About", href: "/om-oss" },
      { name: "Contact", href: "/kontakt" },
    ],
    home: "TriodeLab home",
    homeButton: "Go to the home page",
    mainNavigation: "Main navigation",
    mainMenu: "Main menu",
    menu: "TriodeLab menu",
    openMenu: "Open menu",
    closeMenu: "Close menu",
    closeDropdown: (title) => `Close ${title} menu`,
    dropdownLabel: (title) => `${title} menu`,
    contact: "Contact",
    contactEmailHref:
      "mailto:post@triodelab.no?subject=Enquiry%20from%20triodelab.no&body=Hi%20TriodeLab%2C%0A%0A",
    getStarted: "Get started",
    menuFooter: "TriodeLab — Oslo",
    startProject: "Start a project",
  },
};
