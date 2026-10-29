'use client';

import { useState } from 'react';
import Link from 'next/link';
import HeroSection from '@/components/implementation/HeroSection';
import ServicesSection from '@/components/implementation/ServicesSection';
import ProcessSection from '@/components/implementation/ProcessSection';
import TechContactSection from '@/components/implementation/TechContactSection';

export default function ImplementeringPage() {
  return (
    <div className="min-h-screen">
      {/* Navbar */}
      <nav className="fixed top-0 left-0 right-0 p-6 md:p-10 flex justify-between items-center z-40 bg-transparent pointer-events-none">
        <Link href="/intro" className="text-sm md:text-base font-bold tracking-tight z-50 pointer-events-auto mix-blend-difference text-white md:mix-blend-normal md:text-black">
          TRIODELAB
        </Link>

        {/* Desktop Links */}
        <div className="hidden md:flex items-center gap-10 text-[11px] font-medium tracking-wide pointer-events-auto">
          <Link href="/projects" className="hover:text-gray-500 transition-colors uppercase">PROSJEKTER</Link>
          <Link href="/implementation" className="hover:text-gray-500 transition-colors uppercase">IMPLEMENTERING</Link>
          <Link href="/intro" className="hover:text-gray-500 transition-colors uppercase">HJEM</Link>
        </div>

        <div className="hidden md:flex items-center gap-6 text-[11px] font-medium tracking-wide pointer-events-auto">
          <a href="mailto:post@triodelab.no" className="hover:text-gray-500 transition-colors uppercase">
            E-POST
          </a>
          <a 
            href="https://www.triodelab.no/kontakt" 
            className="border border-black/10 px-6 py-2.5 rounded-full hover:bg-black hover:text-white transition-all duration-300 uppercase bg-white/50 backdrop-blur-sm"
          >
            KONTAKT OSS
          </a>
        </div>
      </nav>

      <HeroSection />
      <ServicesSection />
      <ProcessSection />
      <TechContactSection />
    </div>
  );
}
