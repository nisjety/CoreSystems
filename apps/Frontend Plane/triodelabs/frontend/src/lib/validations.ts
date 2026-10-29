import { z } from "zod";
import type { Locale, Localized } from "./i18n/locales";

const contactValidationMessages: Localized<{
  companyTooLong: string;
  emailInvalid: string;
  emailTooLong: string;
  messageTooLong: string;
  messageTooShort: string;
  nameTooLong: string;
  nameTooShort: string;
  projectTypeRequired: string;
}> = {
  nb: {
    nameTooShort: "Navn må være minst 2 tegn",
    nameTooLong: "Navn kan ikke være mer enn 100 tegn",
    emailInvalid: "Ugyldig e-postadresse",
    emailTooLong: "E-postadressen er for lang",
    companyTooLong: "Bedriftsnavnet er for langt",
    projectTypeRequired: "Vennligst velg prosjekttype",
    messageTooShort: "Beskrivelse må være minst 10 tegn",
    messageTooLong: "Beskrivelse kan ikke være mer enn 1000 tegn",
  },
  en: {
    nameTooShort: "Name must be at least 2 characters",
    nameTooLong: "Name cannot be more than 100 characters",
    emailInvalid: "Enter a valid email address",
    emailTooLong: "The email address is too long",
    companyTooLong: "The company name is too long",
    projectTypeRequired: "Please choose what your enquiry is about",
    messageTooShort: "Please write at least 10 characters",
    messageTooLong: "The description cannot be more than 1000 characters",
  },
};

// Contact Form Schema. The rules are identical in every language; only the
// error messages shown in the form change.
export function getContactFormSchema(locale: Locale) {
  const messages = contactValidationMessages[locale];

  return z.object({
    name: z
      .string()
      .trim()
      .min(2, messages.nameTooShort)
      .max(100, messages.nameTooLong),
    email: z
      .string()
      .trim()
      .email(messages.emailInvalid)
      .max(254, messages.emailTooLong),
    company: z.string().trim().max(200, messages.companyTooLong).optional(),
    projectType: z.enum(
      [
        "webutvikling",
        "design",
        "konsultasjon",
        "ehandel",
        "app",
        "integrasjoner",
        "ki",
        "seo",
        "annonsering",
        "annet",
      ],
      { required_error: messages.projectTypeRequired },
    ),
    budget: z
      .enum(["under-50k", "50k-150k", "150k-500k", "over-500k"])
      .optional(),
    message: z
      .string()
      .trim()
      .min(10, messages.messageTooShort)
      .max(1000, messages.messageTooLong),
  });
}

export const contactFormSchema = getContactFormSchema("nb");

// Newsletter Schema
export const newsletterSchema = z.object({
  email: z.string().email("Ugyldig e-postadresse"),
});

// Type exports
export type ContactFormData = z.infer<typeof contactFormSchema>;
export type NewsletterData = z.infer<typeof newsletterSchema>;
