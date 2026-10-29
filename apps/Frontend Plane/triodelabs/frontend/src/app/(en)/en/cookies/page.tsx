import { LegalDocumentPage } from "@/features/legal/legal-document-page";
import { legalDocumentsByLocale } from "@/features/legal/legal-content-en";
import { createPageMetadata } from "@/lib/seo";

const document = legalDocumentsByLocale.en.cookies;

export const metadata = createPageMetadata({
  title: `${document.title} | TriodeLab`,
  description: document.description,
  path: document.path,
  locale: "en",
});

export default function CookiesPage() {
  return (
    <LegalDocumentPage document={document} locale="en" showCookiePreferences />
  );
}
