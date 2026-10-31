"use client";

import React, { useState, useEffect } from 'react';
import IntroHeader from '@/components/intro/IntroHeader';
import HeroSection from '@/components/intro/HeroSection';
import AboutSection, { ABOUT_CHAPTER_RANGES } from '@/components/intro/AboutSection';
import ServicesSection from '@/components/intro/ServicesSection';
import ProjectsSection from '@/components/intro/ProjectsSection';
import StructureSection from '@/components/intro/StructureSection';
import ContactSection from '@/components/intro/ContactSection';
import IntroSidebar, { MAIN_SECTIONS } from '@/components/intro/IntroSidebar';

export default function IntroPage() {
  const [activeSection, setActiveSection] = useState("hero");
  const [aboutSubIndex, setAboutSubIndex] = useState(0);
  const [servicesSubIndex, setServicesSubIndex] = useState(0);
  const [projectsSubIndex, setProjectsSubIndex] = useState(0);
  const headerOffset = 80;
  const activeSectionRef = React.useRef("hero");

  // Track active section using offset-aware section positions
  useEffect(() => {
    let ticking = false;

    const getSectionY = (id: string) => {
      const el = document.getElementById(id);
      if (!el) return Number.POSITIVE_INFINITY;
      return el.getBoundingClientRect().top + window.scrollY - headerOffset;
    };

    const updateActiveSection = () => {
      const currentY = window.scrollY + 4;
      let currentSection = MAIN_SECTIONS[0]?.id ?? "hero";

      for (const section of MAIN_SECTIONS) {
        const sectionY = getSectionY(section.id);
        if (currentY >= sectionY) {
          currentSection = section.id;
        } else {
          break;
        }
      }

      if (activeSectionRef.current !== currentSection) {
        activeSectionRef.current = currentSection;
        setActiveSection(currentSection);
      }

      ticking = false;
    };

    const onScroll = () => {
      if (ticking) return;
      ticking = true;
      window.requestAnimationFrame(updateActiveSection);
    };

    updateActiveSection();
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);

    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
    };
  }, [headerOffset]);

  const handleNavigate = (sectionId: string, subIndex?: number) => {
    const el = document.getElementById(sectionId);
    if (el) {
      if (sectionId === "about" && subIndex !== undefined) {
        const container = document.getElementById('about-scroll-container');
        if (container && ABOUT_CHAPTER_RANGES[subIndex]) {
          const [start] = ABOUT_CHAPTER_RANGES[subIndex];
          const scrollHeight = container.scrollHeight - window.innerHeight;
          const containerTop = container.getBoundingClientRect().top + window.scrollY;
          const targetScroll = containerTop + (start * scrollHeight) - headerOffset;

          window.scrollTo({
            top: Math.max(0, targetScroll),
            behavior: 'smooth'
          });
        }
      } else if (sectionId === "services" && subIndex !== undefined) {
        const targetScrollProgress = subIndex === 0 ? 0.35 : subIndex === 1 ? 0.65 : 0.95;
        const rect = el.getBoundingClientRect();
        const absoluteTop = rect.top + window.scrollY;
        // height is 400vh -> 4 * window.innerHeight
        const sectionHeight = 4 * window.innerHeight; 
        const targetScroll = absoluteTop + (targetScrollProgress * (sectionHeight - window.innerHeight));

        window.scrollTo({
          top: targetScroll,
          behavior: 'smooth'
        });
      } else if (sectionId === "projects" && subIndex !== undefined) {
         // Project cards are spaced vertically. Rough estimate based on gap-32 and min-h-screen cards.
         // Or finding the card element if possible. But they are inside the component.
         // Let's assume standard scroll.
         const cardElements = document.querySelectorAll('#projects .group'); // The cards have distinct classes?
         // This is brittle.
         // Simple fallback:
         const projectHeight = window.innerHeight; // min-h-[90vh] + gap
         // Scroll to relative offset
         const targetScroll = (el.getBoundingClientRect().top + window.scrollY) - headerOffset + (subIndex * projectHeight);
          window.scrollTo({
          top: targetScroll,
          behavior: 'smooth'
        });
      } else {
        const targetTop = el.getBoundingClientRect().top + window.scrollY - headerOffset;
        window.scrollTo({
          top: Math.max(0, targetTop),
          behavior: 'smooth',
        });
      }
    }
  };

  return (
    <main className="min-h-screen text-foreground relative">
      <IntroHeader />
      
      {/* Global Sidebar */}
      <IntroSidebar 
        activeSection={activeSection} 
        subIndex={
          activeSection === 'about' ? aboutSubIndex :
          activeSection === 'services' ? servicesSubIndex :
          activeSection === 'projects' ? projectsSubIndex :
          0
        }
        onNavigate={handleNavigate}
      />

      <div id="hero">
        <HeroSection />
      </div>
      
      <section id="about" className="relative border-t border-border">
        {/* About Section (reused from About Page) */}
        <div className="mx-auto w-full">
          <AboutSection onChapterChange={setAboutSubIndex} />
        </div>
      </section>

      <div id="services">
        <ServicesSection onIndexChange={setServicesSubIndex} />
      </div>
      
      <div id="projects">
        <ProjectsSection onIndexChange={setProjectsSubIndex} />
      </div>

      <div id="structure">
        <StructureSection />
      </div>

      <div id="contact">
        <ContactSection />
      </div>

      {/* Intro Footer? Optional */}
    </main>
  );
}
