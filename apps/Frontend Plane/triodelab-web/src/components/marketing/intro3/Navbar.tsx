"use client";

import Link from "next/link";
import { Search, Menu } from "lucide-react";
import { useContext } from "react";
import { ThemeContext } from "./ThemeController";

export function Navbar() {
    const { isDark } = useContext(ThemeContext);

    return (
        <nav className={`fixed top-0 left-0 right-0 p-6 md:px-12 md:py-8 flex justify-between items-center z-50 transition-colors duration-700 ${isDark ? 'text-white' : 'text-[#282A22]'}`}>
            {/* Stingray-style text logo */}
            <Link href="/intro3" className="text-xl md:text-2xl font-light tracking-wider hover:opacity-70 transition-opacity uppercase">
                Triodelab
            </Link>

            {/* Minimalist Right Navigation (Search + Menu) */}
            <div className="flex items-center gap-8">
                <button className="flex items-center gap-2 hover:opacity-70 transition-opacity">
                    <Search className="w-5 h-5 stroke-[1.5]" />
                </button>
                <button className="flex items-center gap-2 hover:opacity-70 transition-opacity">
                    <span className="hidden md:block text-xs font-light uppercase tracking-[0.2em] mt-0.5">Meny</span>
                    <Menu className="w-6 h-6 stroke-[1.5]" />
                </button>
            </div>
        </nav>
    );
}
