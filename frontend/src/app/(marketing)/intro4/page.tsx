import { Navbar } from "@/components/marketing/intro4/Navbar";
import { Hero } from "@/components/marketing/intro4/Hero";
import { MediaCards } from "@/components/marketing/intro4/MediaCards";
import { Footer } from "@/components/marketing/intro4/Footer";

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
