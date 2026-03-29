import * as React from "react"

import { cn } from "@/components/chat/lib/utils"

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "file:text-gray-700 placeholder:text-gray-400 selection:bg-blue-100 selection:text-blue-900 bg-gray-50 flex h-10 w-full min-w-0 rounded-xl border-0 px-4 py-2.5 text-base shadow-sm transition-all outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm text-gray-800",
        "focus:bg-white focus:ring-2 focus:ring-blue-500 focus:shadow-[0_4px_24px_rgba(0,0,0,0.08)]",
        "hover:bg-white hover:shadow-[0_4px_24px_rgba(0,0,0,0.08)]",
        className
      )}
      {...props}
    />
  )
}

export { Input }