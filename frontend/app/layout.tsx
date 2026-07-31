import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Trampantojo — Threat Intelligence para América Latina",
  description:
    "Plataforma de inteligencia de amenazas: verificá dominios, URLs, IPs y hashes contra indicadores de compromiso reportados por el CSIRT y la comunidad de seguridad.",
  keywords: ["threat intelligence", "IoC", "CSIRT", "phishing", "seguridad", "América Latina"],
  openGraph: {
    title: "Trampantojo — Threat Intelligence",
    description: "Verificá indicadores de compromiso en tiempo real.",
    type: "website",
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="es"
      className={`${geistSans.variable} ${geistMono.variable}`}
    >
      <body className="page-wrapper">{children}</body>
    </html>
  );
}
