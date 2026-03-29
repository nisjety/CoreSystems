'use client';

import Link from 'next/link';
import { Bell, LifeBuoy, ShieldCheck } from 'lucide-react';

const footerLinks = [
  {
    href: '/overview',
    label: 'Overview',
    icon: ShieldCheck,
  },
  {
    href: '/notifications',
    label: 'Notifications',
    icon: Bell,
  },
  {
    href: '/helpdesk',
    label: 'Support',
    icon: LifeBuoy,
  },
] as const;

export function DashboardFooter() {
  return (
    <footer className="shrink-0 px-6 pb-5 pt-6 md:px-8 md:pb-7 md:pt-8">
      <div
        className="h-px"
        style={{
          background:
            'linear-gradient(90deg, rgba(21,31,109,0) 0%, rgba(21,31,109,0.24) 10%, rgba(78,96,173,0.24) 90%, rgba(21,31,109,0) 100%)',
        }}
      />

      <div className="flex flex-col items-center gap-3 pb-1 pt-5 text-center md:gap-4 md:pt-6">
        <p className="text-[12px] font-medium tracking-[0.2em] text-[#6F7380] uppercase">
          CoreSystem dashboard workspace
        </p>
        <div className="flex items-center gap-5 text-[#6B6F7B]">
          {footerLinks.map(({ href, label, icon: Icon }) => (
            <Link
              key={href}
              href={href}
              aria-label={label}
              title={label}
              className="inline-flex items-center justify-center rounded-full text-[#6B6F7B] transition-colors duration-150 hover:text-[#262B38] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/10"
            >
              <Icon className="h-[17px] w-[17px]" strokeWidth={1.9} />
            </Link>
          ))}
        </div>
      </div>
    </footer>
  );
}