"use client";

import { useState } from "react";
import { ArrowRight, Sparkles, ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { AnimatePresence, m } from "framer-motion";

const NAV_ITEMS = [
    { id: "products", label: "Products" },
    { id: "solutions", label: "Solutions" },
    { id: "resources", label: "Resources" },
    { id: "about", label: "About" }
];

export function TeamSwitcher() {
    const [activeTab, setActiveTab] = useState<string | null>(null);

    // Close dropdown when mouse leaves the entire nav area
    const handleMouseLeave = () => setActiveTab(null);

    return (
        <section className="w-full bg-transparent border-b border-[#282A22]/10 relative z-40 transition-colors">
            {/* Banner equivalent */}
            <div className="bg-[#282A22] text-[#E7E7E6] w-full py-3 px-4 flex justify-center items-center gap-2 hover:bg-black transition-colors cursor-pointer text-sm font-light tracking-wide">
                <Sparkles className="w-4 h-4" />
                <span>Upgrade your digital strategy — join Triodelab today</span>
                <ArrowRight className="w-4 h-4 ml-1" />
            </div>

            {/* Sticky Navigation Area */}
            <div className="sticky top-[89px] z-40 w-full" onMouseLeave={handleMouseLeave}>
                <div className="w-full max-w-7xl mx-auto px-6 hidden lg:flex items-center justify-between h-[80px]">
                    <div className="flex items-center gap-8 w-full">
                        {/* Logo substitute if needed, or just navigation array */}
                        <div className="font-light text-xl tracking-widest text-[#282A22] uppercase">
                            Triodelab
                        </div>

                        <ul className="flex items-center justify-center flex-1 gap-2 space-x-2 mr-32">
                            {NAV_ITEMS.map((item) => (
                                <li key={item.id} className="relative">
                                    <button
                                        onMouseEnter={() => setActiveTab(item.id)}
                                        className={cn(
                                            "group inline-flex items-center justify-center whitespace-nowrap outline-none transition-colors duration-200 h-10 px-5 rounded-full font-light text-[15px] tracking-wide",
                                            activeTab === item.id
                                                ? "bg-[#282A22]/10 text-[#282A22]"
                                                : "text-[#282A22]/60 hover:bg-[#282A22]/5 hover:text-[#282A22]"
                                        )}
                                    >
                                        {item.label}
                                        <ChevronDown className={cn(
                                            "w-4 h-4 ml-1 transition-transform duration-200",
                                            activeTab === item.id ? "rotate-180 text-[#282A22]" : "text-[#282A22]/40"
                                        )} />
                                    </button>
                                </li>
                            ))}
                        </ul>
                    </div>
                </div>

                {/* Dropdown Content Area */}
                <div className="absolute top-[80px] left-0 right-0 w-full flex justify-center">
                    <AnimatePresence>
                        {activeTab && (
                            <m.div
                                initial={{ opacity: 0, y: -5 }}
                                animate={{ opacity: 1, y: 0 }}
                                exit={{ opacity: 0, y: -5 }}
                                transition={{ duration: 0.15, ease: "easeOut" }}
                                className="w-full max-w-7xl rounded-2xl bg-white shadow-[0_8px_30px_rgb(0,0,0,0.08)] border border-slate-200 overflow-hidden mx-6 relative"
                            >
                                <div className="p-8">
                                    {/* Content switches based on active tab */}
                                    <div className="grid grid-cols-3 gap-8">

                                        {/* Demo content for demonstration */}
                                        <div className="col-span-1 border-r border-[#282A22]/10 pr-8">
                                            <div className="flex flex-col gap-2 mb-6">
                                                <h3 className="text-xl font-light tracking-wide text-[#282A22]">{activeTab.charAt(0).toUpperCase() + activeTab.slice(1)}</h3>
                                                <p className="text-[#282A22]/60 font-light text-sm">Manage your business digital footprint at every stage of growth.</p>
                                            </div>
                                            <a href="#" className="inline-flex items-center justify-center px-4 py-2 bg-[#282A22]/5 hover:bg-[#282A22]/10 text-[#282A22] font-light rounded-full text-sm transition-colors group">
                                                Try the Demo
                                                <ArrowRight className="w-4 h-4 ml-2 group-hover:translate-x-1 transition-transform" />
                                            </a>
                                        </div>

                                        <div className="col-span-2 grid grid-cols-2 gap-x-12 gap-y-8">
                                            <div>
                                                <p className="text-xs font-semibold text-[#282A22]/40 uppercase tracking-[0.15em] mb-4">Core Offerings</p>
                                                <ul className="space-y-4">
                                                    <li>
                                                        <a href="#" className="group flex flex-col gap-1">
                                                            <span className="text-[#282A22] font-light tracking-wide group-hover:text-blue-600 transition-colors flex items-center">
                                                                Checking & Savings
                                                                <ArrowRight className="w-4 h-4 ml-2 opacity-0 -translate-x-2 group-hover:opacity-100 group-hover:translate-x-0 transition-all text-blue-600" />
                                                            </span>
                                                            <span className="text-sm text-[#282A22]/50 font-light">Secure foundations.</span>
                                                        </a>
                                                    </li>
                                                    <li>
                                                        <a href="#" className="group flex flex-col gap-1">
                                                            <span className="text-[#282A22] font-light tracking-wide group-hover:text-blue-600 transition-colors flex items-center">
                                                                Working Capital
                                                                <ArrowRight className="w-4 h-4 ml-2 opacity-0 -translate-x-2 group-hover:opacity-100 group-hover:translate-x-0 transition-all text-blue-600" />
                                                            </span>
                                                            <span className="text-sm text-[#282A22]/50 font-light">Grow without limits.</span>
                                                        </a>
                                                    </li>
                                                </ul>
                                            </div>

                                            <div>
                                                <p className="text-xs font-semibold text-[#282A22]/40 uppercase tracking-[0.15em] mb-4">Integrations</p>
                                                <ul className="space-y-4">
                                                    <li>
                                                        <a href="#" className="group flex flex-col gap-1">
                                                            <span className="text-[#282A22] font-light tracking-wide group-hover:text-blue-600 transition-colors flex items-center">
                                                                Accounting Automations
                                                                <ArrowRight className="w-4 h-4 ml-2 opacity-0 -translate-x-2 group-hover:opacity-100 group-hover:translate-x-0 transition-all text-blue-600" />
                                                            </span>
                                                        </a>
                                                    </li>
                                                </ul>
                                            </div>
                                        </div>

                                    </div>
                                </div>
                            </m.div>
                        )}
                    </AnimatePresence>
                </div>
            </div>
        </section>
    );
}
