import { WebSolutionsPage } from "@/features/services/web-solutions-page";
import { webSolutionsSeo } from "@/features/services/service-seo";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: webSolutionsSeo.title,
  description: webSolutionsSeo.description,
  path: webSolutionsSeo.canonical,
  image: webSolutionsSeo.image,
  imageAlt: webSolutionsSeo.name,
});

export default function Page() {
  return <WebSolutionsPage locale="nb" />;
}
