"use client";

import * as React from "react";

// Definitions of sections for the global sidebar
export interface PageSection {
  id: string;
  label: string;
  subSections?: { id: string; label: string; range: [number, number] }[];
}

export const MAIN_SECTIONS: PageSection[] = [
  { id: "hero", label: "Hjem" },
  { 
    id: "about", 
    label: "Om",
    // Sub-sections logic is handled dynamically, but we define the structure here
    subSections: [
      { id: "intro", label: "Om Oss", range: [0, 0] },
      { id: "abdifatah", label: "Frontend", range: [0, 0] },
      { id: "ima", label: "Backend", range: [0, 0] },
      { id: "jack", label: "Fullstack", range: [0, 0] },
    ]
  },
  { 
    id: "services", 
    label: "Tjenester",
    subSections: [
      { id: "overview", label: "Våre tjenester", range: [0, 0] },
      { id: "advisory", label: "Strategisk Rådgivning", range: [0, 0] },
      { id: "implementation", label: "Teknisk Implementering", range: [0, 0] },
      { id: "growth", label: "Digital Vekst", range: [0, 0] },
      { id: "support", label: "Kontinuerlig Support", range: [0, 0] },
    ]
  },
  { 
    id: "projects", 
    label: "Prosjekter",
    subSections: [
      { id: "hottest", label: "Se", range: [0, 0] },
      { id: "andros", label: "Vår", range: [0, 0] },
      { id: "australian", label: "Klassisk", range: [0, 0] },
      { id: "special", label: "Utstilling", range: [0, 0] },
    ]
  },
  { id: "structure", label: "Struktur" },
  { id: "contact", label: "Kontakt" },
];

interface IntroSidebarProps {
  activeSection: string;
  subIndex: number; // Generic sub-index for any section
  onNavigate: (sectionId: string, subIndex?: number) => void;
}

export default function IntroSidebar({ activeSection, subIndex, onNavigate }: IntroSidebarProps) {
  return (
    <div className="fixed left-8 top-1/2 -translate-y-1/2 z-50 hidden xl:block">
      <div className="flex flex-col gap-3">
        {MAIN_SECTIONS.map((section) => {
          const isActive = activeSection === section.id;
          
          return (
            <div key={section.id} className="flex flex-col">
              {/* Main Section Link */}
              <button
                onClick={() => onNavigate(section.id)}
                className={`group flex items-center gap-3 transition-all ${
                  isActive 
                    ? "opacity-100 text-[#111111]" 
                    : "opacity-100 text-[#FF2E63] hover:text-[#111111]"
                }`}
              >
                <div className="relative flex items-center w-8">
                  <div
                    className={`h-px transition-all duration-300 bg-current absolute left-0 ${
                      isActive ? "w-8" : "w-4 group-hover:w-6"
                    }`}
                  />
                </div>
                <span className={`text-xs font-medium uppercase tracking-widest ${isActive ? "font-bold" : ""}`}>
                  {section.label}
                </span>
              </button>

              {/* Sub-sections (for any active section with subSections) */}
              {isActive && section.subSections && (
                <div className="ml-8 mt-2 flex flex-col gap-2 border-l border-[#111111]/20 pl-4 transition-all duration-500">
                  {section.subSections.map((sub, idx) => {
                    const isSubActive = idx === subIndex;
                    return (
                      <button
                        key={sub.id}
                        onClick={(e) => {
                          e.stopPropagation();
                          onNavigate(section.id, idx);
                        }}
                        className={`text-left text-[10px] uppercase tracking-wider transition-colors ${
                          isSubActive
                            ? "font-bold text-[#111111] opacity-100"
                            : "text-[#FF2E63] hover:text-[#111111]"
                        }`}
                      >
                        {sub.label}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
