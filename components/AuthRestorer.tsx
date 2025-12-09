"use client";

import { useEffect, useState, useRef } from "react";
import { useSession } from "next-auth/react";
import { useRouter, usePathname } from "next/navigation";

const PERSISTENT_TOKEN_KEY = "jeopardy_persistent_token";
const PUBLIC_PATHS = ["/login", "/register"];

export default function AuthRestorer({
  children,
}: {
  children: React.ReactNode;
}) {
  const { data: session, status, update } = useSession();
  const [isRestoring, setIsRestoring] = useState(true);
  const restorationAttempted = useRef(false);
  const router = useRouter();
  const pathname = usePathname();

  useEffect(() => {
    async function restoreSession() {
      // Wait for initial session check to complete
      if (status === "loading") return;

      // Already authenticated - store/refresh the persistent token
      if (status === "authenticated" && session?.user?.id) {
        setIsRestoring(false);
        // Fetch and store persistent token for future use (non-blocking)
        fetch("/api/auth/persistent-token")
          .then((res) => res.ok ? res.json() : null)
          .then((data) => {
            if (data?.token) {
              localStorage.setItem(PERSISTENT_TOKEN_KEY, data.token);
            }
          })
          .catch(() => {});
        return;
      }

      // Not authenticated - try to restore from persistent token
      if (status === "unauthenticated" && !restorationAttempted.current) {
        restorationAttempted.current = true;

        const persistentToken = localStorage.getItem(PERSISTENT_TOKEN_KEY);

        if (!persistentToken) {
          setIsRestoring(false);
          return;
        }

        try {
          // Attempt to restore session using persistent token
          const response = await fetch("/api/auth/persistent-token", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ token: persistentToken }),
          });

          if (response.ok) {
            // Session restored via cookie set by the API
            // Trigger NextAuth to re-check the session
            await update();
            // Refresh the page data
            router.refresh();
          } else {
            // Token invalid or expired, clear it
            localStorage.removeItem(PERSISTENT_TOKEN_KEY);
          }
        } catch (error) {
          console.error("Error restoring session:", error);
        }

        setIsRestoring(false);
      } else if (status === "unauthenticated") {
        setIsRestoring(false);
      }
    }

    restoreSession();
  }, [status, session, update, router]);

  // Show loading state only on protected pages during restoration
  if (isRestoring && !PUBLIC_PATHS.includes(pathname)) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-900">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500 mx-auto mb-4"></div>
          <p className="text-gray-400">Loading...</p>
        </div>
      </div>
    );
  }

  return <>{children}</>;
}

// Export for use in logout to clear the persistent token
export function clearPersistentToken() {
  if (typeof window !== "undefined") {
    localStorage.removeItem(PERSISTENT_TOKEN_KEY);
  }
}
