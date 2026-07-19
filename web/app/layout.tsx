import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "SharpClap | Agent-Native Mailing Lists",
  description:
    "SharpClap is the agent-native command center for mailing-list-cli: broadcasts, templates, segmentation, reporting, and clean unsubscribe handling.",
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
