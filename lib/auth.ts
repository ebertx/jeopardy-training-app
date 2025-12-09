import { NextAuthOptions } from "next-auth";
import CredentialsProvider from "next-auth/providers/credentials";
import bcrypt from "bcryptjs";
import { prisma } from "./prisma";
import { randomUUID } from "crypto";

export const authOptions: NextAuthOptions = {
  providers: [
    CredentialsProvider({
      name: "Credentials",
      credentials: {
        email: { label: "Email", type: "email" },
        password: { label: "Password", type: "password" }
      },
      async authorize(credentials) {
        if (!credentials?.email || !credentials?.password) {
          return null;
        }

        const user = await prisma.users.findUnique({
          where: { email: credentials.email }
        });

        if (!user) {
          return null;
        }

        const isPasswordValid = await bcrypt.compare(
          credentials.password,
          user.password_hash
        );

        if (!isPasswordValid) {
          return null;
        }

        // Check if user is approved
        if (!user.approved) {
          throw new Error("Your account is pending approval. You'll receive an email once approved.");
        }

        // Create a persistent session in the database
        const sessionToken = randomUUID();
        const expires = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000); // 30 days

        await prisma.auth_sessions.create({
          data: {
            sessionToken,
            userId: user.id,
            expires,
          },
        });

        return {
          id: user.id.toString(),
          email: user.email,
          name: user.username,
          role: user.role,
          sessionToken, // Pass to JWT callback
        };
      }
    })
  ],
  session: {
    strategy: "jwt",
    maxAge: 30 * 24 * 60 * 60, // 30 days
    updateAge: 24 * 60 * 60, // Refresh session every 24 hours when active
  },
  cookies: {
    sessionToken: {
      name: process.env.NODE_ENV === 'production'
        ? `__Secure-next-auth.session-token`
        : `next-auth.session-token`,
      options: {
        httpOnly: true,
        sameSite: 'lax',
        path: '/',
        secure: process.env.NODE_ENV === 'production',
        maxAge: 30 * 24 * 60 * 60, // 30 days
      },
    },
    callbackUrl: {
      name: process.env.NODE_ENV === 'production'
        ? `__Secure-next-auth.callback-url`
        : `next-auth.callback-url`,
      options: {
        httpOnly: true,
        sameSite: 'lax',
        path: '/',
        secure: process.env.NODE_ENV === 'production',
        maxAge: 30 * 24 * 60 * 60, // 30 days
      },
    },
    csrfToken: {
      name: process.env.NODE_ENV === 'production'
        ? `__Host-next-auth.csrf-token`
        : `next-auth.csrf-token`,
      options: {
        httpOnly: true,
        sameSite: 'lax',
        path: '/',
        secure: process.env.NODE_ENV === 'production',
      },
    },
  },
  pages: {
    signIn: "/login",
  },
  callbacks: {
    async jwt({ token, user }) {
      if (user) {
        token.id = user.id;
        token.username = user.name;
        token.role = user.role;
        token.dbSessionToken = (user as any).sessionToken;
      }

      // Validate database session still exists and is not expired
      if (token.dbSessionToken) {
        const dbSession = await prisma.auth_sessions.findUnique({
          where: { sessionToken: token.dbSessionToken as string },
        });

        if (!dbSession || dbSession.expires < new Date()) {
          // Session expired or deleted, force re-login
          return { ...token, error: "SessionExpired" };
        }

        // Refresh session expiry on activity (every 24 hours)
        const oneDayAgo = new Date(Date.now() - 24 * 60 * 60 * 1000);
        if (dbSession.expires < new Date(Date.now() + 29 * 24 * 60 * 60 * 1000)) {
          // Less than 29 days left, extend it
          await prisma.auth_sessions.update({
            where: { sessionToken: token.dbSessionToken as string },
            data: { expires: new Date(Date.now() + 30 * 24 * 60 * 60 * 1000) },
          });
        }
      }

      return token;
    },
    async session({ session, token }) {
      if (token.error === "SessionExpired") {
        // Force client to re-authenticate
        return { ...session, error: "SessionExpired" };
      }

      if (session.user) {
        session.user.id = token.id as string;
        session.user.username = token.username as string;
        session.user.role = token.role as string;
      }
      return session;
    }
  },
  events: {
    async signOut({ token }) {
      // Clean up database session on logout
      if (token?.dbSessionToken) {
        await prisma.auth_sessions.delete({
          where: { sessionToken: token.dbSessionToken as string },
        }).catch(() => {
          // Ignore if already deleted
        });
      }
    },
  },
};
