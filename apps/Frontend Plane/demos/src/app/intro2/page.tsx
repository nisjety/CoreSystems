'use client';

import { useState } from 'react';
import Link from 'next/link';
import { ArrowUpRight, X, ChevronDown, Check, Smartphone, Monitor, Code } from 'lucide-react';
import { cn } from '@/lib/utils';
import { AnimatePresence, motion } from 'framer-motion';

const WorkModal = () => (
    <div className="grid grid-cols-1 md:grid-cols-2 gap-4 p-4 md:p-8">
        {[
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d9a2389a96025c1cb4_community%20app.png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e064fd2efe8e60820645f9_dashboard-1.png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d843629cf6600f4d48_dashboard.png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d9937ba2778c1b4aec_roasters%202.png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d8a1d590815357bf63_perfect%20roast%202.png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d9ecc6441985cea84d_mental%20health%20(3).png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d928e19e693ab06105_mental%20health%20app%20(1).png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d938b64e3bbbdf91b7_mental%20health%20app%20(2).png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d98d4ce33501e8cafe_Quotes%20App%201.png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d8463d2760f87554ba_Quotes%20App%202.png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d8af5752f6c7a209c5_certora%201.png",
            "https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e063d812defb2609fdd955_certora%202.png"
        ].map((src, i) => (
            <div key={i} className="rounded-xl overflow-hidden bg-gray-50 aspect-auto">
                <img src={src} alt={`Work sample ${i + 1}`} className="w-full h-full object-cover" />
            </div>
        ))}
    </div>
);

const ServicesModal = () => {
    const [openItem, setOpenItem] = useState<string | null>("mobile");

    return (
        <div className="p-6 md:p-12 max-w-4xl mx-auto">
            <div className="mb-12">
                <h2 className="text-4xl font-bold mb-4">Services</h2>
                <p className="text-gray-600 text-lg mb-6 max-w-2xl">
                    We team up with founders and startups to bring their ideas to life—whether it&apos;s a simple landing page, a complete website, or mobile and web apps.
                </p>
                <div className="flex flex-wrap gap-4">
                     <span className="bg-gray-100 px-4 py-2 rounded-full text-sm font-medium">No template work</span>
                     <span className="bg-gray-100 px-4 py-2 rounded-full text-sm font-medium">Deep-dive into your product and ideas</span>
                </div>
            </div>

            <div className="space-y-4">
                 <div className="border border-gray-200 rounded-2xl overflow-hidden">
                    <button 
                        onClick={() => setOpenItem(openItem === "mobile" ? null : "mobile")}
                        className="w-full flex items-center justify-between p-6 bg-white hover:bg-gray-50 transition-colors"
                    >
                        <div className="flex items-center gap-4">
                             <div className="p-2 bg-gray-100 rounded-lg"><Smartphone className="w-5 h-5" /></div>
                             <span className="font-semibold text-lg">Mobile & desktop app design</span>
                        </div>
                        <ChevronDown className={cn("w-5 h-5 transition-transform", openItem === "mobile" ? "rotate-180" : "")} />
                    </button>
                    {openItem === "mobile" && (
                        <div className="p-6 pt-0 bg-white">
                            <p className="text-gray-600 mb-6 ml-[60px]">Whether it&apos;s starting fresh or brushing up what you already have. We&apos;ve worked on simple habit trackers to full-scale trading platforms.</p>
                             <div className="rounded-xl overflow-hidden bg-gray-50 ml-[60px]">
                                <img src="https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e4dfafaa8988f379a7d113_moblie-desk.png" className="w-full" alt="Mobile design" />
                            </div>
                        </div>
                    )}
                 </div>

                 <div className="border border-gray-200 rounded-2xl overflow-hidden">
                    <button 
                        onClick={() => setOpenItem(openItem === "web" ? null : "web")}
                        className="w-full flex items-center justify-between p-6 bg-white hover:bg-gray-50 transition-colors"
                    >
                        <div className="flex items-center gap-4">
                             <div className="p-2 bg-gray-100 rounded-lg"><Monitor className="w-5 h-5" /></div>
                             <span className="font-semibold text-lg">Website design</span>
                        </div>
                         <ChevronDown className={cn("w-5 h-5 transition-transform", openItem === "web" ? "rotate-180" : "")} />
                    </button>
                    {openItem === "web" && (
                        <div className="p-6 pt-0 bg-white">
                            <p className="text-gray-600 mb-6 ml-[60px]">Every page is purposefully designed with clear goals, whether it&apos;s raising awareness or driving conversions.</p>
                             <div className="rounded-xl overflow-hidden bg-gray-50 ml-[60px]">
                                <img src="https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e4dfbf094585887d921aa1_web-landing-pages.png" className="w-full" alt="Web design" />
                            </div>
                        </div>
                    )}
                 </div>

                 <div className="border border-gray-200 rounded-2xl overflow-hidden">
                    <button 
                        onClick={() => setOpenItem(openItem === "dev" ? null : "dev")}
                        className="w-full flex items-center justify-between p-6 bg-white hover:bg-gray-50 transition-colors"
                    >
                         <div className="flex items-center gap-4">
                             <div className="p-2 bg-gray-100 rounded-lg"><Code className="w-5 h-5" /></div>
                             <span className="font-semibold text-lg">Web development</span>
                        </div>
                         <ChevronDown className={cn("w-5 h-5 transition-transform", openItem === "dev" ? "rotate-180" : "")} />
                    </button>
                    {openItem === "dev" && (
                        <div className="p-6 pt-0 bg-white">
                            <div className="ml-[60px]">
                                <h4 className="font-medium mb-3">Recent Projects</h4>
                                <div className="grid grid-cols-2 gap-3 mb-6">
                                    {["Unfiltered Supplements", "RWAF", "Octane Security", "Petra Security"].map(project => (
                                        <div key={project} className="flex items-center justify-between p-3 border rounded-lg text-sm bg-gray-50 hover:bg-gray-100 cursor-pointer">
                                            {project}
                                            <ArrowUpRight className="w-3 h-3 text-gray-400" />
                                        </div>
                                    ))}
                                </div>
                                <p className="text-gray-600 mb-6">We build our websites on Webflow, sticking to high industry standards like Client First.</p>
                                <div className="rounded-xl overflow-hidden bg-gray-50">
                                    <img src="https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e4dfeb88cf2b18b800b0c8_webflow-dev.png" className="w-full" alt="Webflow dev" />
                                </div>
                            </div>
                        </div>
                    )}
                 </div>
            </div>
        </div>
    );
};

const AboutModal = () => (
    <div className="p-6 md:p-12 max-w-4xl mx-auto">
        <h2 className="text-4xl font-bold mb-6">About</h2>
        <p className="text-gray-600 text-lg mb-12 max-w-2xl">
            We&apos;re a three-person team with a thing for startups. Between us, we&apos;ve spent 5 years diving into the Dev world.
        </p>
        
        <div className="grid grid-cols-1 md:grid-cols-3 gap-8 mb-16">
            {/* Person 1 */}
            <div className="space-y-4">
                <div className="aspect-square bg-gray-100 rounded-2xl overflow-hidden rotate-2 hover:rotate-0 transition-transform duration-300">
                    <img src="https://images.unsplash.com/photo-1531384441138-2736e62e0919?w=500&h=500&fit=crop" className="w-full h-full object-cover" alt="Ima da Costa" />
                </div>
                <div className="text-center">
                    <div className="font-semibold text-lg">Ima da Costa</div>
                    <div className="text-gray-500 text-sm">Backend Dev</div>
                    <div className="text-gray-400 text-xs mt-1">🇳🇴 Norway</div>
                </div>
            </div>
            {/* Person 2 */}
            <div className="space-y-4">
                 <div className="aspect-square bg-gray-100 rounded-2xl overflow-hidden hover:rotate-0 transition-transform duration-300">
                    <img src="https://images.unsplash.com/photo-1587723958656-ee042cc565a1?w=500&h=500&fit=crop" className="w-full h-full object-cover" alt="Abdifatah Hassan" />
                </div>
                <div className="text-center">
                    <div className="font-semibold text-lg">Abdifatah Hassan</div>
                    <div className="text-gray-500 text-sm">Web and Design Dev</div>
                    <div className="text-gray-400 text-xs mt-1">🇳🇴 Norway</div>
                </div>
            </div>
            {/* Person 3 */}
            <div className="space-y-4">
                 <div className="aspect-square bg-gray-100 rounded-2xl overflow-hidden -rotate-2 hover:rotate-0 transition-transform duration-300">
                    <img src="https://images.unsplash.com/photo-1552374196-1ab2a1c593e8?w=500&h=500&fit=crop" className="w-full h-full object-cover" alt="Jack Delamou" />
                </div>
                <div className="text-center">
                    <div className="font-semibold text-lg">Jack Delamou</div>
                    <div className="text-gray-500 text-sm">Frontend Dev</div>
                    <div className="text-gray-400 text-xs mt-1">🇳🇴 Norway</div>
                </div>
            </div>
        </div>

        <div className="bg-gray-50 rounded-3xl p-8 mb-12">
            <h3 className="text-xl font-bold mb-8">Summary</h3>
            <div className="space-y-8">
                <div className="flex gap-4">
                    <div className="mt-1"><div className="w-5 h-5 bg-blue-500 rounded-full flex items-center justify-center"><Check className="w-3 h-3 text-white" /></div></div>
                    <div>
                        <div className="font-semibold">At our core, we&apos;re product people</div>
                        <div className="text-gray-600">This means we don&apos;t just design; we craft products with a keen eye on user experience and market fit</div>
                    </div>
                </div>
                <div className="flex gap-4">
                    <div className="mt-1"><div className="w-5 h-5 bg-blue-500 rounded-full flex items-center justify-center"><Check className="w-3 h-3 text-white" /></div></div>
                    <div>
                         <div className="font-semibold">We design with clear goals in mind</div>
                        <div className="text-gray-600">We create designs that don&apos;t just look good but perform, driving user engagement and contributing to your growth</div>
                    </div>
                </div>
                 <div className="flex gap-4">
                    <div className="mt-1"><div className="w-5 h-5 bg-blue-500 rounded-full flex items-center justify-center"><Check className="w-3 h-3 text-white" /></div></div>
                    <div>
                         <div className="font-semibold">Details matter to us</div>
                        <div className="text-gray-600">We believe the little things can make a big difference</div>
                    </div>
                </div>
            </div>
        </div>

         <div className="flex justify-center gap-4">
             <a href="mailto:contact@theoutline.com" className="bg-black text-white px-8 py-3 rounded-full hover:bg-gray-800 transition-colors">Email</a>
             <a href="https://cal.com/the-outline-design" target="_blank" className="bg-gray-100 text-black px-8 py-3 rounded-full hover:bg-gray-200 transition-colors">Book a call</a>
        </div>
    </div>
);

const PlansModal = () => (
    <div className="p-6 md:p-12 max-w-4xl mx-auto">
        <div className="text-center mb-12">
            <h2 className="text-2xl font-medium text-gray-600 mb-6">Before it&apos;s getting serious, make sure you check all our boxes:</h2>
            <div className="bg-gray-50 rounded-2xl p-6 text-left space-y-4 max-w-2xl mx-auto">
                {["You genuinely care about your product and how it impacts your users", 
                  "You’re in regular conversations with your users, and their feedback directly shapes your product",
                  "Your goal isn’t to mimic your competition, you’re aiming for something uniquely yours"].map((item, i) => (
                    <div key={i} className="flex gap-3">
                         <div className="mt-1 min-w-5 h-5 border-2 border-gray-300 rounded flex items-center justify-center" />
                         <span className="text-gray-700">{item}</span>
                    </div>
                ))}
            </div>
        </div>

        <h3 className="text-3xl font-bold text-center mb-8">Pricing</h3>
        
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-12">
            {/* Plan 1 */}
            <div className="bg-white border rounded-2xl p-6 hover:shadow-lg transition-shadow">
                <div className="inline-block bg-gray-100 px-3 py-1 rounded-full text-xs font-semibold mb-4">Flat monthly fee</div>
                <div className="mb-2"><span className="text-3xl font-bold">$6,990</span>/month</div>
                <p className="text-gray-500">Best if you&apos;re looking for steady and ongoing design work.</p>
            </div>
            {/* Plan 2 */}
            <div className="bg-white border rounded-2xl p-6 hover:shadow-lg transition-shadow">
                 <div className="inline-block bg-blue-50 text-blue-600 px-3 py-1 rounded-full text-xs font-semibold mb-4">Webflow development</div>
                 <div className="mb-2"><span className="text-3xl font-bold">$2,990</span>/month</div>
                 <p className="text-gray-500">For ongoing support and consistent updates to your existing Webflow site.</p>
            </div>
        </div>
         {/* Plan 3 */}
            <div className="bg-black text-white rounded-2xl p-6 text-center mb-12">
                 <div className="inline-block bg-white/20 px-3 py-1 rounded-full text-xs font-semibold mb-4">Project based</div>
                 <div className="mb-2"><span className="text-3xl font-bold">Custom</span></div>
                 <p className="text-gray-400">Perfect for a bigger one-time thing.</p>
            </div>

        <div className="flex gap-6 items-start bg-gray-50 p-6 rounded-2xl mb-8">
            <div className="shrink-0 p-2 bg-gray-200 rounded-lg">
                <span className="font-bold">Note</span>
            </div>
            <p className="text-gray-600 text-sm leading-relaxed">
            We&apos;re selective about our partnerships—rarely taking on more than 3-4 projects at once. Quality matters to us, and that means staying focused.
            <br/><br/>
            We&apos;re not just about executing ideas; we&apos;ll challenge them, push back, and ensure your users are as happy as you are.
            <br/><br/>
            If you&apos;re ready for a straightforward, collaborative approach, let&apos;s talk.
            </p>
        </div>

         <div className="flex justify-center gap-4">
             <a href="mailto:contact@theoutline.com" className="bg-black text-white px-8 py-3 rounded-full hover:bg-gray-800 transition-colors">Email</a>
             <a href="https://cal.com/the-outline-design" target="_blank" className="bg-gray-100 text-black px-8 py-3 rounded-full hover:bg-gray-200 transition-colors">Book a call</a>
        </div>
    </div>
);

export default function IntroPage() {
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const [activeModal, setActiveModal] = useState<string | null>(null);

  return (
    <div className="min-h-screen text-black font-sans selection:bg-black selection:text-white">
      {/* Navbar */}
      <nav className="fixed top-0 left-0 right-0 p-6 md:p-10 flex justify-between items-center z-40 bg-transparent pointer-events-none">
           <div className="text-sm md:text-base font-bold tracking-tight z-50 pointer-events-auto mix-blend-difference text-white md:mix-blend-normal md:text-black">
               TRIODELAB
           </div>

        {/* Desktop Links */}
                <div className="hidden md:flex items-center gap-10 text-[11px] font-medium tracking-wide pointer-events-auto">
                      <button onClick={() => setActiveModal('work')} className="hover:text-gray-500 transition-colors uppercase">PROSJEKTER</button>
                    <button onClick={() => setActiveModal('services')} className="hover:text-gray-500 transition-colors uppercase">TJENESTER</button>
                    <button onClick={() => setActiveModal('about')} className="hover:text-gray-500 transition-colors uppercase">OM OSS</button>
                    <button onClick={() => setActiveModal('plans')} className="hover:text-gray-500 transition-colors uppercase">PLANER</button>
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

        {/* Mobile Toggle */}
        <button 
            className="md:hidden text-sm font-medium uppercase z-50 relative pointer-events-auto mix-blend-difference text-white"
            onClick={() => setIsMenuOpen(!isMenuOpen)}
        >
            {isMenuOpen ? (
                <div className="bg-white text-black p-2 rounded-full"><X className="w-5 h-5"/></div>
            ) : 'MENY'}
        </button>
      </nav>

       {/* Mobile Menu Overlay */}
       <AnimatePresence>
       {isMenuOpen && (
        <motion.div 
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 z-40 bg-[#FDFCF8] text-black pt-32 px-6"
        >
            <div className="flex flex-col gap-8 text-4xl font-light tracking-tighter">
                <button className="text-left" onClick={() => { setActiveModal('work'); setIsMenuOpen(false); }}>FÅ RÅDGIVNING</button>
                <button className="text-left" onClick={() => { setActiveModal('services'); setIsMenuOpen(false); }}>TJENESTER</button>
                <button className="text-left" onClick={() => { setActiveModal('about'); setIsMenuOpen(false); }}>OM OSS</button>
                <button className="text-left" onClick={() => { setActiveModal('plans'); setIsMenuOpen(false); }}>PLANER</button>
                <div className="h-px bg-black/10 w-full my-4"></div>
                <Link href="/auth/sign-in" onClick={() => setIsMenuOpen(false)} className="text-lg uppercase font-medium">LOGG INN</Link>
                <Link href="/auth/sign-in" className="text-lg uppercase font-medium" onClick={() => setIsMenuOpen(false)}>BOOK EN SAMTALE</Link>
            </div>
        </motion.div>
       )}
       </AnimatePresence>


            {/* Main Content */}
            <main className="pt-28 md:pt-32 px-4 w-full max-w-[980px] mx-auto min-h-screen flex flex-col items-center justify-start pb-16">
        
                {/* Hero Section */}
                <div className="mt-10 md:mt-14 mb-10 md:mb-12 text-center max-w-[700px]">
                    <h1 className="text-2xl md:text-3xl font-semibold tracking-tight leading-snug text-black">
                        Digital transformasjon som faktisk fungerer
                    </h1>
                    <p className="mt-3 text-sm md:text-base text-[#9CA3AF]">
                        Vi leverer digital transformasjon og teknisk rådgivning med hands-on erfaring og full åpenhet.
                    </p>
                </div>

                {/* Grid / Content - 4 Cards */}
                <div className={`grid grid-cols-2 lg:grid-cols-4 gap-4 md:gap-5 w-full max-w-[760px] transition-all duration-500 ${activeModal ? 'scale-[0.98] opacity-50 blur-sm pointer-events-none' : ''}`}>
            
                        {/* Card 1: Work */}
                        <a className="group cursor-pointer" href="projects">
                            <div className="rounded-2xl border border-black/10 bg-white p-4 md:p-5 shadow-[0_1px_0_rgba(0,0,0,0.04)] hover:shadow-[0_4px_16px_rgba(0,0,0,0.06)] transition-shadow">
                                        <div className="relative aspect-3/4 rounded-xl overflow-hidden bg-[#F2F2F2]">
                                             <img 
                                                 src="https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e4de903e6acfdfc0e4a395_work-thumbnail.png"
                                                 alt="Work"
                                                 className="w-full h-full object-cover"
                                             />
                                        </div>
                                        <div className="mt-3 text-center text-sm font-semibold">Prosjekter</div>
                                </div>
                        </a>

                        {/* Card 2: Services */}
                        <a className="group cursor-pointer" href="implementation">
                                <div className="rounded-2xl border border-black/10 bg-white p-4 md:p-5 shadow-[0_1px_0_rgba(0,0,0,0.04)] hover:shadow-[0_4px_16px_rgba(0,0,0,0.06)] transition-shadow">
                                        <div className="relative aspect-3/4 rounded-xl overflow-hidden bg-[#F2F2F2]">
                                             <img 
                                                 src="https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e9b414b8428bcce621e898_Work%20Thumbnail.png"
                                                 alt="Services"
                                                 className="w-full h-full object-cover"
                                             />
                                        </div>
                                        <div className="mt-3 text-center text-sm font-semibold">Implementering</div>
                                </div>
                        </a>

                        {/* Card 3: About */}
                        <div className="group cursor-pointer" onClick={() => setActiveModal('about')}>
                                <div className="rounded-2xl border border-black/10 bg-white p-4 md:p-5 shadow-[0_1px_0_rgba(0,0,0,0.04)] hover:shadow-[0_4px_16px_rgba(0,0,0,0.06)] transition-shadow">
                                        <div className="relative aspect-3/4 rounded-xl overflow-hidden bg-[#F2F2F2]">
                                             <img 
                                                 src="https://cdn.prod.website-files.com/653437233b349b44eda5816c/65ac380540612cf79cbccdfd_about-thumbnail.png"
                                                 alt="About"
                                                 className="w-full h-full object-cover"
                                             />
                                        </div>
                                        <div className="mt-3 text-center text-sm font-semibold">Vekst</div>
                                </div>
                        </div>

                        {/* Card 4: Plans */}
                        <div className="group cursor-pointer" onClick={() => setActiveModal('plans')}>
                                <div className="rounded-2xl border border-black/10 bg-white p-4 md:p-5 shadow-[0_1px_0_rgba(0,0,0,0.04)] hover:shadow-[0_4px_16px_rgba(0,0,0,0.06)] transition-shadow">
                                        <div className="relative aspect-3/4 rounded-xl overflow-hidden bg-[#F2F2F2]">
                                             <img 
                                                 src="https://cdn.prod.website-files.com/653437233b349b44eda5816c/65e9b4140e2b223eb5ff792f_Plans%20Thumbnail.png"
                                                 alt="Plans"
                                                 className="w-full h-full object-cover"
                                             />
                                        </div>
                                        <div className="mt-3 text-center text-sm font-semibold">Support</div>
                                </div>
                        </div>
            
                </div>

                {/* Bottom CTA */}
                <div className="mt-10 md:mt-14 flex flex-col items-center gap-3">
                    <div className="flex items-center gap-2">
                        <a href="mailto:post@triodelab.no" className="rounded-full bg-black text-white px-5 py-2 text-sm font-semibold">Email</a>
                        <a href="https://www.triodelab.no/kontakt" className="rounded-full bg-white border border-black/10 px-5 py-2 text-sm font-semibold">Kontakt oss</a>
                        <a href="https://www.triodelab.no/prosjekter" className="rounded-full bg-white border border-black/10 px-5 py-2 text-sm font-semibold">Få rådgivning</a>
                    </div>
                    <p className="text-xs text-gray-500">Oslo, Norge • © 2026 TriodeLab</p>
                </div>
            </main>

      {/* Modals Overlay */}
      <AnimatePresence>
        {activeModal && (
            <>
                <motion.div
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    onClick={() => setActiveModal(null)}
                    className="fixed inset-0 bg-black/20 backdrop-blur-sm z-50 cursor-pointer"
                />
                <motion.div
                    initial={{ y: "100%" }}
                    animate={{ y: "0%" }}
                    exit={{ y: "100%" }}
                    transition={{ type: "spring", damping: 30, stiffness: 300, mass: 0.8 }}
                    className="fixed bottom-0 left-0 right-0 z-50 bg-white rounded-t-4xl md:rounded-t-[3rem] shadow-2xl overflow-hidden h-[92vh] md:h-[96vh] cursor-default"
                >
                    <div className="h-full overflow-y-auto relative bg-white">
                         <div className="sticky top-0 right-0 flex justify-end p-6 md:p-8 z-20 pointer-events-none">
                            <button 
                                onClick={() => setActiveModal(null)}
                                className="pointer-events-auto p-3 bg-[#F5F5F7] rounded-full hover:bg-[#E5E5E7] transition-colors shadow-sm"
                            >
                                <X className="w-6 h-6 text-black" />
                            </button>
                        </div>
                        
                        <div className="pb-20 pt-2 px-1">
                            {activeModal === 'work' && <WorkModal />}
                            {activeModal === 'services' && <ServicesModal />}
                            {activeModal === 'about' && <AboutModal />}
                            {activeModal === 'plans' && <PlansModal />}
                        </div>
                    </div>
                </motion.div>
            </>
        )}
      </AnimatePresence>
    </div>
  );
}
