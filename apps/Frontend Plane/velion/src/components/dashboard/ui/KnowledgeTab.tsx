interface KnowledgeTabProps {
  onNavigate?: (href: string) => void;
}

export function KnowledgeTab({ onNavigate }: KnowledgeTabProps) {
  const links = [
    { label: 'Datakilder', href: '/knowledge/sources' },
    { label: 'Dokumenter', href: '/knowledge/documents' },
    { label: 'Oversikt', href: '/knowledge' },
  ];

  return (
    <div className="rounded-2xl border border-[#E5E0D8] bg-white p-6 shadow-sm">
      <p className="text-center text-[14px] text-[#A09890]">
        Utforsk og administrer din kunnskapsbase
      </p>
      <div className="mt-4 flex flex-wrap justify-center gap-2">
        {links.map(({ label, href }) => (
          <button
            key={href}
            type="button"
            onClick={() => { if (onNavigate) onNavigate(href); }}
            className="rounded-xl bg-[#F5F0EA] px-4 py-2 text-[13px] font-medium text-[#5A504A] transition-colors hover:bg-[#EDE8DF]"
          >
            {label}
          </button>
        ))}
      </div>
    </div>
  );
}
