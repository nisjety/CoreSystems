import type { Metadata } from 'next'
import { Navbar } from "@/components/marketing/intro4/Navbar";
import { Hero } from "@/components/marketing/intro4/Hero";
import { MediaCards } from "@/components/marketing/intro4/MediaCards";
import { Footer } from "@/components/marketing/intro4/Footer";

export const metadata: Metadata = {
  title: 'Triodelab – Teknologi & Digital Design',
  description: 'Triodelab bygger skreddersydde digitale produkter med fokus på moderne teknologi og brukervennlig design – fra konsept til ferdig løsning.',
}

export default function Intro4Page() {
    return (
        <main className="w-full flex flex-col items-center justify-start min-h-screen bg-[#E7E7E6] selection:bg-[#282A22] selection:text-[#E7E7E6] font-sans">
            <Navbar />
            <Hero />
            <MediaCards />
            <Footer />
        </main>
    );
}
