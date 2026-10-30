"use client";
import { cn } from "../utils/cn";
import { Menu, X } from "lucide-react";
import {
  motion,
  AnimatePresence,
  useScroll,
  useMotionValueEvent,
} from "framer-motion";

import React, { useRef, useState } from "react";

interface NavbarProps {
  children: React.ReactNode;
  className?: string;
}

interface NavBodyProps {
  children: React.ReactNode;
  className?: string;
  visible?: boolean;
}

interface NavItemsProps {
  items: {
    name: string;
    link: string;
  }[];
  className?: string;
  onItemClick?: () => void;
}

interface MobileNavProps {
  children: React.ReactNode;
  className?: string;
  visible?: boolean;
}

interface MobileNavHeaderProps {
  children: React.ReactNode;
  className?: string;
}

interface MobileNavMenuProps {
  children: React.ReactNode;
  className?: string;
  isOpen: boolean;
  onClose: () => void;
}

export const Navbar = ({ children, className }: NavbarProps) => {
  const ref = useRef<HTMLDivElement>(null);
  const { scrollY } = useScroll({
    target: ref,
    offset: ["start start", "end start"],
  });
  const [visible, setVisible] = useState<boolean>(false);

  useMotionValueEvent(scrollY, "change", (latest) => {
    if (latest > 50) {
      setVisible(true);
    } else {
      setVisible(false);
    }
  });

  return (
    <motion.div
      ref={ref}
      className={cn("fixed inset-x-0 top-0 z-40 w-full", className)}
    >
      {React.Children.map(children, (child) =>
        React.isValidElement(child)
          ? React.cloneElement(
              child as React.ReactElement<{ visible?: boolean }>,
              { visible },
            )
          : child,
      )}
    </motion.div>
  );
};

export const NavBody = ({ children, className, visible }: NavBodyProps) => {
  return (
    <motion.div
      animate={{
        backdropFilter: visible ? "blur(16px)" : "none",
        boxShadow: visible
          ? "0 8px 32px rgba(0, 0, 0, 0.12), 0 2px 6px rgba(0, 0, 0, 0.08), 0 0 0 1px rgba(229, 229, 229, 0.5)"
          : "none",
        y: visible ? 0 : 0,
        borderRadius: visible ? "0px" : "0px",
      }}
      transition={{
        type: "spring",
        stiffness: 400,
        damping: 30,
      }}
      style={{
        background: visible ? "rgba(255, 255, 255, 0.95)" : "transparent",
      }}
      className={cn(
        "relative z-[60] w-full flex-row items-center justify-between self-start backdrop-blur-md px-4 py-4 lg:flex hidden",
        visible && "border-b border-gray-200",
        className,
      )}
    >
      {children}
    </motion.div>
  );
};

// Following Miller's Law: Limit navigation items to avoid cognitive overload
export const NavItems = ({ items, className, onItemClick }: NavItemsProps) => {
  const [hovered, setHovered] = useState<number | null>(null);

  return (
    <motion.div
      onMouseLeave={() => setHovered(null)}
      className={cn(
        "absolute inset-0 hidden flex-1 flex-row items-center justify-center space-x-1 text-sm font-medium text-white/90 transition duration-200 hover:text-white lg:flex lg:space-x-1",
        className,
      )}
    >
      {items.map((item, idx) => (
        <a
          onMouseEnter={() => setHovered(idx)}
          onClick={onItemClick}
          className="relative px-4 py-2 text-white/80 hover:text-white rounded-xl transition-all duration-200 focus:outline-none focus:ring-2 focus:ring-white/30"
          key={`link-${idx}`}
          href={item.link}
        >
          {/* Following Von Restorff Effect: Highlight active/hovered items */}
          {hovered === idx && (
            <motion.div
              layoutId="hovered"
              className="absolute inset-0 h-full w-full rounded-xl bg-white/15 backdrop-blur-sm"
              transition={{ type: "spring", stiffness: 400, damping: 30 }}
            />
          )}
          <span className="relative z-20">{item.name}</span>
        </a>
      ))}
    </motion.div>
  );
};

export const MobileNav = ({ children, className, visible }: MobileNavProps) => {
  return (
    <motion.div
      animate={{
        backdropFilter: visible ? "blur(16px)" : "none",
        boxShadow: visible
          ? "0 8px 32px rgba(0, 0, 0, 0.12), 0 2px 6px rgba(0, 0, 0, 0.08), 0 0 0 1px rgba(229, 229, 229, 0.5)"
          : "none",
        y: visible ? 0 : 0,
        borderRadius: visible ? "0px" : "0px",
      }}
      transition={{
        type: "spring",
        stiffness: 400,
        damping: 30,
      }}
      style={{
        background: visible ? "rgba(255, 255, 255, 0.95)" : "transparent",
      }}
      className={cn(
        "relative z-50 w-full flex flex-col items-center justify-between backdrop-blur-md px-3 py-4 lg:hidden",
        visible && "border-b border-gray-200",
        className,
      )}
    >
      {children}
    </motion.div>
  );
};

export const MobileNavHeader = ({
  children,
  className,
}: MobileNavHeaderProps) => {
  return (
    <div
      className={cn(
        "flex w-full flex-row items-center justify-between px-4",
        className,
      )}
    >
      {children}
    </div>
  );
};

// Following Aesthetic-Usability Effect: Clean, smooth animations enhance perceived usability
export const MobileNavMenu = ({
  children,
  className,
  isOpen,
  onClose,
}: MobileNavMenuProps) => {
  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          initial={{ opacity: 0, y: -10, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: -10, scale: 0.98 }}
          transition={{ 
            type: "spring", 
            stiffness: 500, 
            damping: 30,
            duration: 0.2 
          }}
          className={cn(
            "absolute inset-x-0 top-16 z-50 flex w-full flex-col items-start justify-start gap-6 rounded-2xl backdrop-blur-md px-6 py-8 border border-white/10",
            className,
          )}
          style={{
            background: "oklch(0.292 0.132 269.8 / 0.98)",
            boxShadow: "0 8px 32px rgba(0, 0, 0, 0.12), 0 2px 6px rgba(0, 0, 0, 0.08), 0 0 0 1px rgba(255, 255, 255, 0.1)",
          }}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
};

// Following Fitts's Law: Adequate touch target size for mobile interaction
export const MobileNavToggle = ({
  isOpen,
  onClick,
  isScrolled = false,
}: {
  isOpen: boolean;
  onClick: () => void;
  isScrolled?: boolean;
}) => {
  return (
    <button
      onClick={onClick}
      className={`p-3 rounded-xl transition-all duration-200 focus:outline-none focus:ring-2 shadow-sm hover:shadow-md ${
        isScrolled 
          ? 'hover:bg-white/10 active:bg-white/20 focus:ring-white/30 shadow-md hover:shadow-lg' 
          : 'hover:bg-primary/10 active:bg-primary/20 focus:ring-primary/30'
      }`}
      aria-label={isOpen ? "Close menu" : "Open menu"}
    >
      <motion.div
        animate={{ rotate: isOpen ? 180 : 0 }}
        transition={{ duration: 0.2, ease: "easeInOut" }}
      >
        {isOpen ? (
          <X className={`w-5 h-5 ${isScrolled ? 'text-white' : 'text-black'}`} />
        ) : (
          <Menu className={`w-5 h-5 ${isScrolled ? 'text-white' : 'text-black'}`} />
        )}
      </motion.div>
    </button>
  );
};

export const NavbarLogo = () => {
  return (
    <a
      href="#"
      className="relative z-20 mr-4 flex items-center space-x-3 px-3 py-2 text-sm font-normal text-white hover:bg-white/10 rounded-xl transition-all duration-200"
    >
      <div className="w-8 h-8 bg-white rounded-lg flex items-center justify-center">
        <span className="text-primary font-bold text-sm">S</span>
      </div>
      <span className="font-semibold text-white">Startup</span>
    </a>
  );
};

// Following Goal-Gradient Effect: Clear visual feedback for interactive elements
export const NavbarButton = ({
  href,
  as: Tag = "a",
  children,
  className,
  variant = "primary",
  ...props
}: {
  href?: string;
  as?: React.ElementType;
  children: React.ReactNode;
  className?: string;
  variant?: "primary" | "secondary" | "dark" | "gradient";
} & (
  | React.ComponentPropsWithoutRef<"a">
  | React.ComponentPropsWithoutRef<"button">
)) => {
  const baseStyles =
    "px-6 py-3 rounded-xl font-semibold relative cursor-pointer hover:-translate-y-0.5 active:translate-y-0 transition-all duration-200 inline-block text-center focus:outline-none focus:ring-2 focus:ring-white/30";

  const variantStyles = {
    primary:
      "bg-white text-primary shadow-[0_8px_32px_rgba(0,0,0,0.12),_0_2px_6px_rgba(0,0,0,0.08)] hover:shadow-[0_12px_40px_rgba(0,0,0,0.15),_0_4px_12px_rgba(0,0,0,0.1)] hover:bg-white/95",
    secondary: "bg-transparent border-2 border-white/20 text-white hover:bg-white/10 hover:border-white/30 shadow-[0_4px_16px_rgba(0,0,0,0.08)]",
    dark: "bg-black/20 text-white shadow-[0_8px_32px_rgba(0,0,0,0.12),_0_2px_6px_rgba(0,0,0,0.08)] hover:bg-black/30 border border-white/10",
    gradient:
      "bg-gradient-to-r from-white to-white/90 text-primary shadow-[0_8px_32px_rgba(0,0,0,0.12),_0_2px_6px_rgba(0,0,0,0.08)] hover:shadow-[0_12px_40px_rgba(0,0,0,0.15),_0_4px_12px_rgba(0,0,0,0.1)] hover:from-white/95 hover:to-white/85",
  };

  return (
    <Tag
      href={href || undefined}
      className={cn(baseStyles, variantStyles[variant], className)}
      {...props}
    >
      {children}
    </Tag>
  );
};