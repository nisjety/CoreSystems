"use client";

export function Footer() {
    return (
        <footer className="w-full px-6 py-24 md:px-12 lg:px-24 bg-[#F2F2F2] text-[#282A22] relative z-10 selection:bg-[#282A22] selection:text-[#E7E7E6]">
            <div className="max-w-[1600px] mx-auto grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-16 font-light text-[16px] leading-relaxed">

                {/* Brand / Col 1 */}
                <div className="flex flex-col gap-6 lg:max-w-xs">
                    <h2 className="text-2xl font-light tracking-wide mb-4">Triodelab</h2>
                    <p className="opacity-80">
                        Intelligent AI solutions for modern businesses. We design and build transformative systems.
                    </p>
                    <a href="mailto:post@triodelab.no" className="hover:opacity-60 transition-opacity underline underline-offset-4">
                        post@triodelab.no
                    </a>
                </div>

                {/* Selskap / Col 2 */}
                <div className="flex flex-col gap-4">
                    <h3 className="text-[12px] font-medium tracking-[0.15em] uppercase mb-4 opacity-50">Selskap</h3>
                    <a href="#" className="hover:opacity-60 transition-opacity">Om oss</a>
                    <a href="#" className="hover:opacity-60 transition-opacity flex items-center justify-between group">
                        Karriere <span className="opacity-40 group-hover:translate-x-1 group-hover:opacity-100 transition-all text-xs">↗</span>
                    </a>
                    <a href="#" className="hover:opacity-60 transition-opacity flex items-center justify-between group">
                        Nyheter <span className="opacity-40 group-hover:translate-x-1 group-hover:opacity-100 transition-all text-xs">↗</span>
                    </a>
                    <a href="#" className="hover:opacity-60 transition-opacity">Kontakt</a>
                </div>

                {/* Tjenester / Col 3 */}
                <div className="flex flex-col gap-4">
                    <h3 className="text-[12px] font-medium tracking-[0.15em] uppercase mb-4 opacity-50">Teknologi</h3>
                    <a href="#" className="hover:opacity-60 transition-opacity">AI Utvikling</a>
                    <a href="#" className="hover:opacity-60 transition-opacity">Systemarkitektur</a>
                    <a href="#" className="hover:opacity-60 transition-opacity">Automasjon</a>
                    <a href="#" className="hover:opacity-60 transition-opacity">Produktdesign</a>
                </div>

                {/* Ressurser / Col 4 */}
                <div className="flex flex-col gap-4">
                    <h3 className="text-[12px] font-medium tracking-[0.15em] uppercase mb-4 opacity-50">Ressurser</h3>
                    <a href="#" className="hover:opacity-60 transition-opacity flex items-center justify-between group">
                        Innsikt & Forskning <span className="opacity-40 group-hover:translate-x-1 group-hover:opacity-100 transition-all text-xs">↗</span>
                    </a>
                    <a href="#" className="hover:opacity-60 transition-opacity flex items-center justify-between group">
                        Dokumentasjon <span className="opacity-40 group-hover:translate-x-1 group-hover:opacity-100 transition-all text-xs">↗</span>
                    </a>
                    <a href="#" className="hover:opacity-60 transition-opacity flex items-center justify-between group">
                        Support <span className="opacity-40 group-hover:translate-x-1 group-hover:opacity-100 transition-all text-xs">↗</span>
                    </a>
                </div>

            </div>

            <div className="max-w-[1600px] mx-auto mt-32 pt-12 border-t border-[#282A22]/10 flex flex-col md:flex-row justify-between items-center text-[11px] uppercase tracking-widest opacity-60 font-medium">
                <p>© 2026 TRIODELAB AS.</p>
                <div className="flex gap-8 mt-6 md:mt-0 lg:ml-auto">
                    <a href="#" className="hover:opacity-100 transition-opacity">Personvern</a>
                    <a href="#" className="hover:opacity-100 transition-opacity">Informasjonskapsler</a>
                </div>
            </div>
        </footer>
    );
}
