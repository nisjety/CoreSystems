import { InsightsPage } from "@/features/insights/insights-page";
import { createPageMetadata } from "@/lib/seo";

export const metadata = createPageMetadata({
  title: "Innsikt og fagblogg om KI, nettbutikk og integrasjoner | TriodeLab",
  description:
    "Guider og fagartikler om KI, nettbutikk, systemintegrasjoner og digital utvikling for virksomheter som vil ta bedre teknologivalg.",
  path: "/innsikt",
});

export default function Page() {
  return <InsightsPage />;
}
