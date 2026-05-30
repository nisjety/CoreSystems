import { promptSuggestions } from "@/features/composer-v2/lib/dashboard-composer-options";

type DashboardComposerSuggestionsProps = {
  onMessageChange: (message: string) => void;
  onSuggestionsOpenChange: (open: boolean) => void;
};

export function DashboardComposerSuggestions({
  onMessageChange,
  onSuggestionsOpenChange,
}: DashboardComposerSuggestionsProps) {
  return (
    <div className="velion-popover velion-popover-up absolute bottom-full left-0 z-[90] mb-2 w-72 rounded-2xl border border-black/[0.06] bg-white p-2 shadow-[0_8px_32px_rgba(0,0,0,0.12)] dark:border-[#2A2C31] dark:bg-[#141516]">
      <p className="px-2 py-1.5 text-[11px] font-medium uppercase tracking-wide text-[#999]">Suggestions</p>
      {promptSuggestions.map((suggestion) => (
        <button
          key={suggestion}
          type="button"
          onClick={() => {
            onMessageChange(suggestion);
            onSuggestionsOpenChange(false);
          }}
          title={suggestion}
          className="w-full rounded-xl px-3 py-2.5 text-left text-[13px] text-[#333] transition-colors hover:bg-black/5 dark:text-[#F7F8F8] dark:hover:bg-white/10"
        >
          {suggestion}
        </button>
      ))}
    </div>
  );
}
