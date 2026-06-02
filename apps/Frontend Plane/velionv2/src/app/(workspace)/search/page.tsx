import type { Metadata } from "next";
import { VelionProductShell } from "@/features/shell-v2/components/VelionProductShell";
import { SearchAnswerView } from "@/features/search-v2/components/SearchAnswerView";
import { requireCompletedOnboarding } from "@/lib/auth/onboarding-access";

export const dynamic = "force-dynamic";

export async function generateMetadata({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}): Promise<Metadata> {
  const { q } = await searchParams;
  const query = typeof q === "string" ? q : "";
  return {
    title: query ? `${query} — Søk | Velion` : "Søk | Velion",
    description: query ? `Søkeresultater for ${query}` : "Velion web search",
  };
}

export default async function SearchPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  await requireCompletedOnboarding("/dashboard");

  const { q } = await searchParams;
  const initialQuery = typeof q === "string" ? q : "";

  return (
    <VelionProductShell activeRoute="/search">
      <SearchAnswerView initialQuery={initialQuery} />
    </VelionProductShell>
  );
}
