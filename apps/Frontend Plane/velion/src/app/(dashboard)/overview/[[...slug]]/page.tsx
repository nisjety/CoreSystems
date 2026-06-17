import { OverviewWorkspacePage } from '@/components/dashboard/product-section-pages';

export default function OverviewSectionPage({ params }: { params: { slug?: string[] } }) {
  return <OverviewWorkspacePage currentViewId={params.slug?.[0]} />;
}
