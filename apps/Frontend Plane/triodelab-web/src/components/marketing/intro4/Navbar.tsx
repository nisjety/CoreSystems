"use client";

import Link from "next/link";
import { Search, Menu } from "lucide-react";

export function Navbar() {
    return (
        <nav className="fixed top-0 left-0 right-0 py-8 px-6 md:px-12 flex justify-between items-center z-50 text-[#282A22] mix-blend-difference selection:bg-[#282A22] selection:text-[#E7E7E6]">
            {/* Minimalist Text Logo */}
            <Link href="/intro4" className="text-xl font-light tracking-wide hover:opacity-70 transition-opacity">
                Triodelab
            </Link>

            {/* Right Navigation Controls */}
            <div className="flex items-center gap-10">
                <button className="flex items-center hover:opacity-70 transition-opacity group">
                    <Search className="w-5 h-5 stroke-[1.5]" />
                </button>
                <button className="flex items-center gap-3 hover:opacity-70 transition-opacity group">
                    <span className="hidden md:block text-xs font-light uppercase tracking-[0.15em] mt-0.5 group-hover:underline underline-offset-4">Meny</span>
                    <Menu className="w-6 h-6 stroke-[1.5]" />
                </button>
            </div>
        </nav>
    );
}
