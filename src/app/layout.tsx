import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "ClaudeWXTray",
  description: "Claude Code WeChat Relay Desktop App",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="zh">
      <body>{children}</body>
    </html>
  );
}
