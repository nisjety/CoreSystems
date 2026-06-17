import Link from 'next/link';
import { ArrowDown, ArrowUp, Minus } from 'lucide-react';
import type { DashboardSnapshot } from '../lib/overview-types';

interface MetricCardProps {
  metric: DashboardSnapshot;
  href?: string;
}

export function MetricCard({ metric, href }: MetricCardProps) {
  const trends = metric.trend;
  const isUp = trends?.direction === 'up';
  const isDown = trends?.direction === 'down';
  const isNeutral = trends?.direction === 'stable';

  const trendColor = isUp ? 'text-green-600' : isDown ? 'text-red-600' : 'text-gray-600';
  const trendBgColor = isUp ? 'bg-green-50' : isDown ? 'bg-red-50' : 'bg-gray-50';
  const Icon = isUp ? ArrowUp : isDown ? ArrowDown : Minus;

  const content = (
    <div className="rounded-[22px] border border-[#E6E8EF] bg-white p-6 hover:shadow-md transition-shadow">
      <div className="flex items-start justify-between">
        <div>
          <p className="text-sm font-medium text-[#707480] mb-2">{metric.label}</p>
          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-bold text-[#2F3138]">{metric.value}</span>
            <span className="text-lg text-[#707480]">{metric.unit}</span>
          </div>
        </div>
        {trends && (
          <div className={`flex items-center gap-1 rounded-lg px-2 py-1 ${trendBgColor}`}>
            <Icon className={`w-4 h-4 ${trendColor}`} />
            <span className={`text-xs font-semibold ${trendColor}`}>{trends.value}%</span>
          </div>
        )}
      </div>
    </div>
  );

  if (href) {
    return <Link href={href}>{content}</Link>;
  }

  return content;
}
