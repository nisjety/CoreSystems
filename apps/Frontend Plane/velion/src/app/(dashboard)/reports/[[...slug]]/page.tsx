import { ReportsDashboard } from '@/components/reports/ReportsDashboard';

export default function ReportsSectionPage({ params }: { params: { slug?: string[] } }) {
  void params;
  return <ReportsDashboard />;
}
