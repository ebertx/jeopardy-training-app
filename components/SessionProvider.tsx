"use client";

import { SessionProvider as NextAuthSessionProvider } from "next-auth/react";
import AuthRestorer from "./AuthRestorer";

export default function SessionProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <NextAuthSessionProvider>
      <AuthRestorer>{children}</AuthRestorer>
    </NextAuthSessionProvider>
  );
}
