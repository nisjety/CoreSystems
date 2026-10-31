"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { motion } from "framer-motion";
import { ChevronDown } from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { type ReactNode, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { ArrowRight } from "@/components/icons";
import { Footer } from "@/components/layout/footer";
import { Navigation } from "@/components/layout/navigation";
import { usePrefersReducedMotion } from "@/lib/use-prefers-reduced-motion";
import type { Locale } from "@/lib/i18n/locales";
import { localizeHref } from "@/lib/i18n/routes";
import { getContactFormSchema, type ContactFormData } from "@/lib/validations";
import { contactContent } from "./contact-page-content";
import { getProjectTypeForSource, type ContactSource } from "./contact-sources";

const fieldClassName =
  "w-full border-0 border-b border-[#CFCFC9] bg-transparent px-0 py-3.5 font-manrope text-[1rem] font-light text-[#161616] outline-none transition-colors placeholder:text-[#969692] hover:border-[#969692] focus:border-[#161616] aria-[invalid=true]:border-[#9D3F3A]";

export function ContactPage({
  configured,
  locale,
  source = "direct",
}: {
  configured: boolean;
  locale: Locale;
  source?: ContactSource;
}) {
  const { form: copy, page, projectTypes } = contactContent[locale];
  const reducedMotion = usePrefersReducedMotion();
  const [feedback, setFeedback] = useState<{
    kind: "success" | "error";
    text: string;
  } | null>(null);
  const submission = useRef<{ payload: string; id: string } | null>(null);
  const honeypot = useRef<HTMLInputElement>(null);
  const form = useForm<ContactFormData>({
    resolver: zodResolver(getContactFormSchema(locale)),
    defaultValues: {
      name: "",
      email: "",
      company: "",
      projectType: getProjectTypeForSource(source),
      budget: undefined,
      message: "",
    },
  });
  const entrance = (delay: number) => ({
    initial: reducedMotion ? false : { opacity: 0, y: 20 },
    animate: { opacity: 1, y: 0 },
    transition: {
      duration: reducedMotion ? 0 : 0.8,
      delay: reducedMotion ? 0 : delay,
      ease: [0.22, 1, 0.36, 1] as const,
    },
  });

  const onSubmit = async (data: ContactFormData) => {
    setFeedback(null);
    const payload = JSON.stringify({
      ...data,
      locale,
      source,
      website: honeypot.current?.value ?? "",
    });
    if (submission.current?.payload !== payload) {
      submission.current = { payload, id: crypto.randomUUID() };
    }
    try {
      const response = await fetch("/api/contact", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          ...JSON.parse(payload),
          submissionId: submission.current.id,
        }),
        signal: AbortSignal.timeout(30_000),
      });
      const result = await response.json();
      if (!response.ok || result.success !== true) {
        setFeedback({
          kind: "error",
          text:
            response.status === 429 ? copy.errorRateLimited : copy.errorGeneric,
        });
        return;
      }
      setFeedback({
        kind: "success",
        text: copy.success,
      });
      form.reset();
      submission.current = null;
    } catch {
      setFeedback({ kind: "error", text: copy.errorGeneric });
    }
  };

  return (
    <>
      <Navigation locale={locale} variant="editorial" />
      <main id="main-content" className="bg-[#FCFCFA] text-[#161616]">
        <section
          aria-labelledby="contact-page-heading"
          className="grid min-h-[100svh] grid-rows-[minmax(0,1fr)_minmax(24rem,48svh)] pt-[76px] lg:grid-cols-[minmax(0,1.5fr)_minmax(28rem,1.15fr)] lg:grid-rows-1"
        >
          <div className="flex min-h-0 flex-col px-[clamp(24px,4vw,56px)] pb-[clamp(40px,5vw,72px)] pt-[clamp(48px,8vh,104px)] lg:pr-[clamp(38px,6vw,104px)]">
            <motion.div {...entrance(0.04)}>
              <p className="m-0 font-label text-[0.7rem] font-light uppercase tracking-[0.14em] text-[#626260]">
                {page.eyebrow}
              </p>
              <h1
                id="contact-page-heading"
                className="hero-title mt-[clamp(32px,6vw,88px)] max-w-[11ch] font-manrope text-[clamp(3.4rem,6vw,7.25rem)] !leading-[0.9] !tracking-[-0.075em]"
              >
                {page.heading}
              </h1>
            </motion.div>

            {configured ? <motion.form
              {...entrance(0.1)}
              aria-labelledby="contact-form-heading"
              className="mt-[clamp(40px,7vh,96px)] grid max-w-[46rem] gap-x-10 gap-y-8 sm:grid-cols-2"
              noValidate
              onSubmit={form.handleSubmit(onSubmit)}
            >
              <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2 sm:col-span-2">
                <h2
                  id="contact-form-heading"
                  className="m-0 font-label text-[0.7rem] font-light uppercase tracking-[0.14em] text-[#626260]"
                >
                  {copy.formHeading}
                </h2>
                <p className="m-0 font-body text-sm font-light text-[#626260]">
                  {page.responseTime}
                </p>
              </div>
              <div aria-hidden="true" className="hidden">
                <label>
                  {copy.honeypot}
                  <input
                    name="website"
                    autoComplete="off"
                    tabIndex={-1}
                    ref={honeypot}
                  />
                </label>
              </div>
              <Field
                error={form.formState.errors.name?.message}
                errorId="contact-name-error"
                label={copy.nameLabel}
                required
              >
                <input
                  {...form.register("name")}
                  aria-describedby={
                    form.formState.errors.name
                      ? "contact-name-error"
                      : undefined
                  }
                  aria-invalid={Boolean(form.formState.errors.name)}
                  aria-required="true"
                  autoComplete="name"
                  className={fieldClassName}
                  placeholder={copy.namePlaceholder}
                  required
                />
              </Field>
              <Field
                error={form.formState.errors.email?.message}
                errorId="contact-email-error"
                label={copy.emailLabel}
                required
              >
                <input
                  {...form.register("email")}
                  aria-describedby={
                    form.formState.errors.email
                      ? "contact-email-error"
                      : undefined
                  }
                  aria-invalid={Boolean(form.formState.errors.email)}
                  aria-required="true"
                  autoComplete="email"
                  className={fieldClassName}
                  placeholder={copy.emailPlaceholder}
                  required
                  type="email"
                />
              </Field>
              <Field
                label={copy.companyLabel}
                error={form.formState.errors.company?.message}
                errorId="contact-company-error"
              >
                <input
                  {...form.register("company")}
                  aria-describedby={
                    form.formState.errors.company
                      ? "contact-company-error"
                      : undefined
                  }
                  aria-invalid={Boolean(form.formState.errors.company)}
                  autoComplete="organization"
                  className={fieldClassName}
                  maxLength={200}
                  placeholder={copy.companyPlaceholder}
                />
              </Field>
              <Field label={copy.projectTypeLabel}>
                <div className="relative">
                  <select
                    {...form.register("projectType")}
                    className={`${fieldClassName} cursor-pointer appearance-none pr-8`}
                  >
                    {projectTypes.map((projectType) => (
                      <option
                        key={projectType.value}
                        value={projectType.value}
                      >
                        {projectType.label}
                      </option>
                    ))}
                  </select>
                  <ChevronDown
                    aria-hidden="true"
                    className="pointer-events-none absolute right-0 top-1/2 size-5 -translate-y-1/2 stroke-[1.4] text-[#626260]"
                  />
                </div>
              </Field>
              <Field
                className="sm:col-span-2"
                error={form.formState.errors.message?.message}
                errorId="contact-message-error"
                label={copy.messageLabel}
                required
              >
                <textarea
                  {...form.register("message")}
                  aria-describedby={
                    form.formState.errors.message
                      ? "contact-message-error"
                      : undefined
                  }
                  aria-invalid={Boolean(form.formState.errors.message)}
                  aria-required="true"
                  className={`${fieldClassName} min-h-32 resize-y`}
                  placeholder={copy.messagePlaceholder}
                  required
                  rows={4}
                />
              </Field>

              <p className="m-0 max-w-[34rem] font-body text-sm leading-[1.5] text-[#626260] sm:col-span-2">
                {copy.privacyBefore}
                <Link
                  className="underline underline-offset-4"
                  href={localizeHref("/personvern", locale)}
                >
                  {copy.privacyLink}
                </Link>
                {copy.privacyAfter}
              </p>
              <div className="mt-2 flex flex-wrap items-center gap-x-7 gap-y-4 sm:col-span-2">
                <button
                  className="site-cta site-cta--ocean disabled:cursor-wait disabled:opacity-60"
                  type="submit"
                  disabled={form.formState.isSubmitting}
                >
                  {form.formState.isSubmitting ? copy.sending : copy.submit}{" "}
                  <ArrowRight aria-hidden="true" className="size-4" />
                </button>
                {feedback && (
                  <p
                    role={feedback.kind === "error" ? "alert" : "status"}
                    className="m-0 font-body text-[0.95rem] text-[#365866]"
                  >
                    {feedback.text}
                  </p>
                )}
              </div>
            </motion.form> : (
              <a
                className="mt-[clamp(40px,7vh,96px)] inline-flex min-h-12 w-fit items-center border-b border-[#161616] font-label text-sm uppercase tracking-[0.1em]"
                href={`mailto:${page.email}`}
              >
                {copy.emailFallback}
              </a>
            )}

            <motion.div
              {...entrance(0.16)}
              className="mt-auto pt-[clamp(56px,9vh,112px)]"
            >
              <p className="m-0 max-w-[19rem] font-body text-[clamp(1.05rem,1.25vw,1.28rem)] font-light leading-[1.38] text-[#454543]">
                {page.introduction}
              </p>
            </motion.div>
          </div>

          <motion.div
            {...entrance(0.12)}
            className="relative min-h-[24rem] overflow-hidden bg-[#DDE6E4] lg:min-h-0"
          >
            <Image
              alt={page.image.alt}
              className="object-cover object-center"
              fill
              priority
              sizes="(min-width: 1024px) 43vw, 100vw"
              src={page.image.src}
            />
            <div
              aria-hidden="true"
              className="absolute inset-0 bg-[#163236]/[0.04]"
            />
            <p className="absolute bottom-5 left-6 m-0 font-label text-[0.68rem] font-light uppercase tracking-[0.13em] text-white [text-shadow:0_1px_12px_rgba(0,0,0,0.28)]">
              {copy.location}
            </p>
          </motion.div>
        </section>

      </main>
      <Footer locale={locale} />
    </>
  );
}

function Field({
  children,
  className,
  error,
  errorId,
  label,
  required = false,
}: {
  children: ReactNode;
  className?: string;
  error?: string;
  errorId?: string;
  label: string;
  required?: boolean;
}) {
  return (
    <div className={`block ${className ?? ""}`}>
      <label className="block">
        <span className="font-label text-[0.7rem] font-light uppercase tracking-[0.11em] text-[#626260]">
          {label}
          {required && (
            <span aria-hidden="true" className="ml-1 text-[#161616]">
              *
            </span>
          )}
        </span>
        {children}
      </label>
      {error && (
        <span
          id={errorId}
          className="mt-2 block font-body text-sm text-[#9D3F3A]"
        >
          {error}
        </span>
      )}
    </div>
  );
}
