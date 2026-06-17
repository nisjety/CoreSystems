import Link from 'next/link';
import type { LucideIcon } from 'lucide-react';

interface EmptyStateProps {
  icon?: LucideIcon;
  title: string;
  description?: string;
  action?: {
    label: string;
    href: string;
  };
}

export function EmptyState({ icon: Icon, title, description, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center py-12 px-4 text-center">
      {Icon && <Icon className="w-12 h-12 text-[#707480] mb-4" />}
      <h3 className="text-lg font-semibold text-[#2F3138] mb-2">{title}</h3>
      {description && <p className="text-sm text-[#707480] mb-4 max-w-md">{description}</p>}
      {action && (
        <Link href={action.href} className="inline-block mt-4 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium">
          {action.label}
        </Link>
      )}
    </div>
  );
}
