import { NextRequest, NextResponse } from "next/server";
import { prisma } from "@/lib/prisma";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { randomUUID } from "crypto";
import { encode } from "next-auth/jwt";

// Validate a persistent token and create a new session
export async function POST(request: NextRequest) {
  try {
    const { token } = await request.json();

    if (!token) {
      return NextResponse.json({ error: "Token required" }, { status: 400 });
    }

    const session = await prisma.auth_sessions.findUnique({
      where: { sessionToken: token },
      include: { user: true },
    });

    if (!session) {
      return NextResponse.json({ error: "Invalid token" }, { status: 401 });
    }

    if (session.expires < new Date()) {
      // Clean up expired session
      await prisma.auth_sessions.delete({ where: { sessionToken: token } });
      return NextResponse.json({ error: "Token expired" }, { status: 401 });
    }

    // Check if user is still approved
    if (!session.user.approved) {
      return NextResponse.json({ error: "Account not approved" }, { status: 403 });
    }

    // Extend session expiry
    const newExpiry = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000);
    await prisma.auth_sessions.update({
      where: { sessionToken: token },
      data: { expires: newExpiry },
    });

    // Create a new JWT for the user
    const jwtToken = await encode({
      token: {
        id: session.user.id.toString(),
        email: session.user.email,
        name: session.user.username,
        username: session.user.username,
        role: session.user.role,
        dbSessionToken: token,
      },
      secret: process.env.NEXTAUTH_SECRET!,
      maxAge: 30 * 24 * 60 * 60, // 30 days
    });

    // Create response with the new session cookie
    const response = NextResponse.json({
      success: true,
      user: {
        id: session.user.id.toString(),
        email: session.user.email,
        username: session.user.username,
        role: session.user.role,
      },
    });

    // Set the session cookie
    const cookieName = process.env.NODE_ENV === 'production'
      ? '__Secure-next-auth.session-token'
      : 'next-auth.session-token';

    response.cookies.set(cookieName, jwtToken, {
      httpOnly: true,
      sameSite: 'lax',
      path: '/',
      secure: process.env.NODE_ENV === 'production',
      maxAge: 30 * 24 * 60 * 60, // 30 days
    });

    return response;
  } catch (error) {
    console.error("Persistent token validation error:", error);
    return NextResponse.json({ error: "Internal error" }, { status: 500 });
  }
}

// Generate a new persistent token for the current user
export async function GET(request: NextRequest) {
  try {
    const session = await getServerSession(authOptions);

    if (!session?.user?.id) {
      return NextResponse.json({ error: "Not authenticated" }, { status: 401 });
    }

    // Check if user already has a valid session token
    const existingSession = await prisma.auth_sessions.findFirst({
      where: {
        userId: parseInt(session.user.id),
        expires: { gt: new Date() },
      },
      orderBy: { expires: 'desc' },
    });

    if (existingSession) {
      return NextResponse.json({ token: existingSession.sessionToken });
    }

    // Create a new persistent token
    const sessionToken = randomUUID();
    const expires = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000);

    await prisma.auth_sessions.create({
      data: {
        sessionToken,
        userId: parseInt(session.user.id),
        expires,
      },
    });

    return NextResponse.json({ token: sessionToken });
  } catch (error) {
    console.error("Error generating persistent token:", error);
    return NextResponse.json({ error: "Internal error" }, { status: 500 });
  }
}
