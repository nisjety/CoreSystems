import { WebSolutionsPage } from "@/features/services/web-solutions-page";
import { webSolutionsSeoEn } from "@/features/services/service-seo-en";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: webSolutionsSeoEn.title,
  description: webSolutionsSeoEn.description,
  path: webSolutionsSeoEn.canonical,
  image: webSolutionsSeoEn.image,
  imageAlt: webSolutionsSeoEn.name,
  locale: "en",
});

export default function Page() {
  return <WebSolutionsPage locale="en" />;
}
