"use client";

import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg text-sm font-medium transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#153845] disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "bg-[#91BDCA] text-[#161616] hover:bg-[#ABD0DA]",
        secondary:
          "border border-[#CAD7DB] bg-white text-[#161616] hover:bg-[#DCECF0]",
        ghost: "text-[#153845] hover:bg-[#DCECF0]",
        outline:
          "border border-[#CAD7DB] bg-transparent text-[#153845] hover:bg-[#DCECF0]",
        link: "text-[#153845] underline-offset-4 hover:underline",
      },
      size: {
        sm: "h-9 px-3 text-xs",
        default: "h-11 px-6 py-3",
        lg: "h-12 px-8 py-4 text-lg",
        xl: "h-14 px-10 py-5 text-xl",
        icon: "h-10 w-10",
      },
    },
    defaultVariants: {
      variant: "primary",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends
    React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";

export { Button, buttonVariants };
