import { contactProjectTypes } from "../../features/contact/contact-page-content";
import type { Locale } from "../i18n/locales";
import type { ContactSubmission } from "./request";

const sourceLabels: Record<ContactSubmission["source"], string> = {
  direct: "Kontaktsiden",
  "web-losninger": "Tjenesteside: Webløsninger",
  nettbutikk: "Tjenesteside: Nettbutikk",
  "organisk-trafikk": "Tjenesteside: Organisk trafikk",
  integrasjoner: "Tjenesteside: Integrasjoner",
  "google-og-meta-ads": "Tjenesteside: Google + Meta Ads",
  "ki-og-automatisering": "Tjenesteside: KI og automatisering",
  "radgivning-og-konsulentbistand": "Tjenesteside: Rådgivning",
};

const languageLabels: Record<Locale, string> = {
  nb: "Norsk",
  en: "Engelsk – svar på engelsk",
};

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function projectLabel(value: ContactSubmission["projectType"]): string {
  return (
    contactProjectTypes.find((type) => type.value === value)?.label ?? value
  );
}

function receivedAt(date: Date): string {
  return new Intl.DateTimeFormat("nb-NO", {
    dateStyle: "long",
    timeStyle: "short",
    timeZone: "Europe/Oslo",
  }).format(date);
}

export function buildContactEmail(
  submission: ContactSubmission,
  date = new Date(),
): { html: string; subject: string; text: string } {
  const project = projectLabel(submission.projectType);
  const source = sourceLabels[submission.source];
  const company = submission.company?.trim() || "";
  const when = receivedAt(date);
  const language = languageLabels[submission.locale ?? "nb"];
  const subject = `Ny henvendelse: ${submission.name} – ${project}`;

  const rows: [string, string][] = [
    ["Navn", escapeHtml(submission.name)],
    [
      "E-post",
      `<a href="mailto:${escapeHtml(submission.email)}" style="color:#161616;text-decoration:underline;">${escapeHtml(submission.email)}</a>`,
    ],
    ["Bedrift", company ? escapeHtml(company) : "Ikke oppgitt"],
    ["Gjelder", escapeHtml(project)],
    ["Kilde", escapeHtml(source)],
    ["Språk", escapeHtml(language)],
  ];

  const rowHtml = rows
    .map(
      ([label, value]) => `
            <tr>
              <td style="padding:14px 0;border-bottom:1px solid #E6E6E2;width:120px;vertical-align:top;font-size:11px;letter-spacing:0.12em;text-transform:uppercase;color:#969692;">${label}</td>
              <td style="padding:14px 0;border-bottom:1px solid #E6E6E2;vertical-align:top;font-size:15px;color:#161616;">${value}</td>
            </tr>`,
    )
    .join("");

  const message = escapeHtml(submission.message).replace(/\r?\n/g, "<br>");
  const replyHref = `mailto:${encodeURIComponent(submission.email)}?subject=${encodeURIComponent(`Sv: Henvendelse til TriodeLab – ${project}`)}`;

  const html = `<!doctype html>
<html lang="nb">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>${escapeHtml(subject)}</title>
  </head>
  <body style="margin:0;padding:0;background:#F2F1ED;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif;">
    <div style="display:none;max-height:0;overflow:hidden;">${escapeHtml(`${submission.name} vil snakke om ${project.toLowerCase()}.`)}</div>
    <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#F2F1ED;">
      <tr>
        <td align="center" style="padding:40px 16px;">
          <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:600px;background:#FFFFFF;border:1px solid #E6E6E2;">
            <tr>
              <td style="padding:28px 36px;border-bottom:1px solid #E6E6E2;">
                <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
                  <tr>
                    <td style="font-size:15px;letter-spacing:0.28em;color:#161616;">TRIODELAB</td>
                    <td align="right" style="font-size:11px;letter-spacing:0.12em;text-transform:uppercase;color:#969692;">Ny henvendelse</td>
                  </tr>
                </table>
              </td>
            </tr>
            <tr>
              <td style="padding:40px 36px 8px;">
                <p style="margin:0 0 10px;font-size:11px;letter-spacing:0.12em;text-transform:uppercase;color:#969692;">${escapeHtml(when)}</p>
                <h1 style="margin:0;font-size:30px;line-height:1.1;font-weight:300;letter-spacing:-0.03em;color:#161616;">${escapeHtml(submission.name)} vil ta en prat.</h1>
                <p style="margin:14px 0 0;font-size:15px;line-height:1.5;color:#626260;">Gjelder: ${escapeHtml(project)}${company ? ` · ${escapeHtml(company)}` : ""}</p>
              </td>
            </tr>
            <tr>
              <td style="padding:24px 36px 8px;">
                <p style="margin:0 0 10px;font-size:11px;letter-spacing:0.12em;text-transform:uppercase;color:#969692;">Melding</p>
                <div style="padding:20px 22px;background:#F7F7F4;border-left:2px solid #161616;font-size:16px;line-height:1.6;color:#161616;">${message}</div>
              </td>
            </tr>
            <tr>
              <td style="padding:24px 36px 8px;">
                <table role="presentation" width="100%" cellpadding="0" cellspacing="0">${rowHtml}
                </table>
              </td>
            </tr>
            <tr>
              <td style="padding:28px 36px 40px;">
                <a href="${replyHref}" style="display:inline-block;padding:14px 22px;background:#161616;color:#FFFFFF;font-size:14px;text-decoration:none;">Svar ${escapeHtml(submission.name.split(" ")[0])} &rarr;</a>
                <p style="margin:16px 0 0;font-size:13px;line-height:1.5;color:#969692;">Du kan også trykke Svar i e-postprogrammet. Svaret går rett til ${escapeHtml(submission.email)}.</p>
              </td>
            </tr>
            <tr>
              <td style="padding:20px 36px;border-top:1px solid #E6E6E2;font-size:12px;line-height:1.5;color:#969692;">
                Sendt fra kontaktskjemaet på triodelab.no · Vi svarer normalt innen én arbeidsdag.
              </td>
            </tr>
          </table>
        </td>
      </tr>
    </table>
  </body>
</html>`;

  const text = [
    `Ny henvendelse fra triodelab.no – ${when}`,
    "",
    `Navn:    ${submission.name}`,
    `E-post:  ${submission.email}`,
    `Bedrift: ${company || "Ikke oppgitt"}`,
    `Gjelder: ${project}`,
    `Kilde:   ${source}`,
    `Språk:   ${language}`,
    "",
    "Melding:",
    submission.message,
    "",
    "Svar direkte på denne e-posten for å svare kunden.",
  ].join("\n");

  return { html, subject, text };
}
