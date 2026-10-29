"use client";

import React, { createContext, useContext, useState, useEffect, useRef } from "react";
import { m, useInView } from "framer-motion";

export const ThemeContext = createContext<{ isDark: boolean; setIsDark: (val: boolean) => void }>({
    isDark: false,
    setIsDark: () => { },
});

export function ThemeProvider({ children }: { children: React.ReactNode }) {
    const [isDark, setIsDark] = useState(true); // Hero is dark initially

    return (
        <ThemeContext.Provider value={{ isDark, setIsDark }}>
            <m.div
                animate={{
                    backgroundColor: isDark ? "#0b1121" : "#E7E7E6",
                    color: isDark ? "#ffffff" : "#282A22"
                }}
                transition={{ duration: 0.6, ease: "easeInOut" }}
                className="min-h-screen font-sans selection:bg-blue-600 selection:text-white transition-colors"
            >
                {children}
            </m.div>
        </ThemeContext.Provider>
    );
}

export function ThemeMarker({ isDarkTheme, children, className }: { isDarkTheme: boolean, children?: React.ReactNode, className?: string }) {
    const ref = useRef(null);
    const isInView = useInView(ref, { margin: "-40% 0px -40% 0px" });
    const { setIsDark } = useContext(ThemeContext);

    useEffect(() => {
        if (isInView) {
            setIsDark(isDarkTheme);
        }
    }, [isInView, isDarkTheme, setIsDark]);

    return (
        <div ref={ref} className={className}>
            {children}
        </div>
    );
}
