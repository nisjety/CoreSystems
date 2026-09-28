import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Teachme — Learn by doing",
  description:
    "A Windows learning companion in development for schools. Guided practice, teacher-reviewed courses and a school-controlled learning environment.",
};
export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
