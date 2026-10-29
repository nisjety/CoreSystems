import { LegalDocumentPage } from "@/features/legal/legal-document-page";
import { legalDocumentsByLocale } from "@/features/legal/legal-content-en";
import { createPageMetadata } from "@/lib/seo";

const document = legalDocumentsByLocale.nb.salesTerms;

export const metadata = createPageMetadata({
  title: `${document.title} | TriodeLab`,
  description: document.description,
  path: document.path,
});

export default function SalesTermsPage() {
  return <LegalDocumentPage document={document} locale="nb" />;
}
