import { HeroSection } from '@/components/marketing/intro3/HeroSection';
import { ProjectsSection } from '@/components/marketing/intro3/ProjectsSection';
import { TeamSwitcher } from '@/components/marketing/intro3/TeamSwitcher';
import { ProductSection } from '@/components/marketing/intro3/ImageSectionPlaceholder';
import { Navbar } from '@/components/marketing/intro3/Navbar';
import { ThemeProvider, ThemeMarker } from '@/components/marketing/intro3/ThemeController';

export default function Intro3Page() {
  return (
    <ThemeProvider>
      <Navbar />
      <main className="w-full flex flex-col items-center justify-start">
        {/* Hero Section - starts light, local logic transitions it to dark on zoom */}
        <ThemeMarker isDarkTheme={false} className="w-full">
          <HeroSection />
        </ThemeMarker>

        {/* TeamSwitcher - sets the theme to light */}
        <ThemeMarker isDarkTheme={false} className="w-full">
          <TeamSwitcher />
        </ThemeMarker>

        {/* ProductSection (Lovart replica) - sets the theme to dark blue */}
        <ThemeMarker isDarkTheme={true} className="w-full">
          <ProductSection />
        </ThemeMarker>

        {/* ProjectsSection (Papertiger replica) - sets the theme back to light */}
        <ThemeMarker isDarkTheme={false} className="w-full">
          <ProjectsSection />
        </ThemeMarker>
      </main>

      {/* Footer - light theme */}
      <ThemeMarker isDarkTheme={false} className="w-full">
        <footer className="w-full px-6 py-24 md:px-12 lg:px-24 bg-[#F2F2F2] text-[#282A22] relative z-10 transition-colors duration-700">
          <div className="max-w-[1600px] mx-auto grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-12 font-light text-[15px]">
            {/* Col 1 */}
            <div className="flex flex-col gap-4">
              <h3 className="text-[11px] font-bold tracking-[0.15em] uppercase mb-2 opacity-80">Selskap</h3>
              <a href="#" className="hover:opacity-60 transition-opacity">Om Triodelab</a>
              <a href="#" className="hover:opacity-60 transition-opacity">Karriere</a>
              <a href="#" className="hover:opacity-60 transition-opacity">Kontakt</a>
            </div>

            {/* Col 2 */}
            <div className="flex flex-col gap-4">
              <h3 className="text-[11px] font-bold tracking-[0.15em] uppercase mb-2 opacity-80">Tjenester</h3>
              <a href="#" className="hover:opacity-60 transition-opacity">AI Utvikling</a>
              <a href="#" className="hover:opacity-60 transition-opacity">Systemarkitektur</a>
              <a href="#" className="hover:opacity-60 transition-opacity">Automasjon</a>
            </div>

            {/* Col 3 */}
            <div className="flex flex-col gap-4">
              <h3 className="text-[11px] font-bold tracking-[0.15em] uppercase mb-2 opacity-80">Ressurser</h3>
              <a href="#" className="hover:opacity-60 transition-opacity flex items-center justify-between group">
                Kundecaser <span className="text-[10px] group-hover:translate-x-1 transition-transform">↗</span>
              </a>
              <a href="#" className="hover:opacity-60 transition-opacity flex items-center justify-between group">
                Github <span className="text-[10px] group-hover:translate-x-1 transition-transform">↗</span>
              </a>
              <a href="#" className="hover:opacity-60 transition-opacity flex items-center justify-between group">
                Blogg <span className="text-[10px] group-hover:translate-x-1 transition-transform">↗</span>
              </a>
            </div>

            {/* Col 4 */}
            <div className="flex flex-col gap-4">
              <h3 className="text-[11px] font-bold tracking-[0.15em] uppercase mb-2 opacity-80">Kontakt</h3>
              <a href="mailto:post@triodelab.no" className="hover:opacity-60 transition-opacity hover:underline">post@triodelab.no</a>
              <p className="mt-4 text-sm opacity-60">Oslo, Norge</p>
            </div>
          </div>
          <div className="max-w-[1600px] mx-auto mt-24 pt-8 border-t border-[#282A22]/10 flex flex-col md:flex-row justify-between items-center text-[10px] uppercase tracking-widest opacity-60 font-semibold">
            <p>© 2026 TRIODELAB AS.</p>
            <div className="flex gap-6 mt-4 md:mt-0">
              <a href="#" className="hover:opacity-100 transition-opacity">Personvern</a>
              <a href="#" className="hover:opacity-100 transition-opacity">Informasjonskapsler</a>
            </div>
          </div>
        </footer>
      </ThemeMarker>
    </ThemeProvider>
  );
}
