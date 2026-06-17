import { PlanBadge } from './PlanBadge';

interface DashboardHeaderProps {
  greeting: string;
  firstName: string;
  onUpgrade?: () => void;
}

export function DashboardHeader({
  greeting,
  firstName,
  onUpgrade,
}: DashboardHeaderProps) {
  return (
    <div className="px-4 pb-6 pt-10">
      <div className="mx-auto flex w-full max-w-5xl flex-col items-center">
        <div className="mb-4 w-full max-w-[720px]">
          <PlanBadge onUpgrade={onUpgrade} />
        </div>

        <h1 className="mb-8 w-full max-w-[720px] text-[52px] font-[450] leading-none tracking-tight text-[#1A1A1A] sm:text-[64px]">
          {greeting}, {firstName}
        </h1>
      </div>
    </div>
  );
}
