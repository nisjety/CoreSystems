import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { InsightArticlePage } from "@/features/insights/insight-article-page";
import { findInsight, insights } from "@/features/insights/insights-content";
import { createPageMetadata } from "@/lib/seo";

type PageProps = {
  params: Promise<{ slug: string }>;
};

export function generateStaticParams() {
  return insights.map(({ slug }) => ({ slug }));
}

export async function generateMetadata({
  params,
}: PageProps): Promise<Metadata> {
  const { slug } = await params;
  const insight = findInsight(slug);

  if (!insight) {
    return {};
  }

  return createPageMetadata({
    title: `${insight.title} | TriodeLab`,
    description: insight.description,
    path: insight.canonical,
    image: insight.hero.image,
    imageAlt: insight.hero.alt,
    type: "article",
  });
}

export default async function Page({ params }: PageProps) {
  const { slug } = await params;
  const insight = findInsight(slug);

  if (!insight) {
    notFound();
  }

  return <InsightArticlePage insight={insight} />;
}
