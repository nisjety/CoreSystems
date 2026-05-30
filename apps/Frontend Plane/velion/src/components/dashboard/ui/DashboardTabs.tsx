type ActiveTab = 'Chat' | 'Søk' | 'Kunnskap';

interface DashboardTabsProps {
  activeTab: ActiveTab;
  onTabChange: (tab: ActiveTab) => void;
}

export function DashboardTabs({ activeTab, onTabChange }: DashboardTabsProps) {
  const tabs: ActiveTab[] = ['Chat', 'Søk', 'Kunnskap'];

  return (
    <div className="sticky top-0 z-10 px-4 pb-2 pt-4">
      <div className="mx-auto flex w-full max-w-5xl justify-center">
        <div className="inline-flex items-center gap-0.5 rounded-full bg-black/6 p-1">
        {tabs.map((tab) => (
          <button
            key={tab}
            onClick={() => onTabChange(tab)}
            className={`rounded-full px-5 py-1.5 text-[13px] font-medium transition-all duration-150 ${
              activeTab === tab
                ? 'bg-white text-[#1A1A1A] shadow-sm'
                : 'text-[#6B6560] hover:text-[#1A1A1A]'
            }`}
          >
            {tab}
          </button>
        ))}
        </div>
      </div>
    </div>
  );
}

export type { ActiveTab };
