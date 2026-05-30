interface PlanBadgeProps {
  plan?: string;
  onUpgrade?: () => void;
  showUpgrade?: boolean;
}

export function PlanBadge({
  plan = 'Pro Plan',
  onUpgrade,
  showUpgrade = true,
}: PlanBadgeProps) {
  return (
    <span className="inline-flex items-center gap-1.5 rounded-full border border-[#E5E0D8] bg-white px-3.5 py-1 text-[12.5px] font-medium text-[#6B6560]">
      {plan}
      {showUpgrade && (
        <>
          <span className="text-[#D4C9BF]">·</span>
          <button
            onClick={onUpgrade}
            className="font-semibold text-[#E8853D] hover:underline"
          >
            Upgrade
          </button>
        </>
      )}
    </span>
  );
}
