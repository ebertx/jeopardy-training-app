import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

export async function GET() {
  const session = await getServerSession(authOptions);

  if (!session?.user || session.user.role !== "admin") {
    return NextResponse.json({ error: "Unauthorized" }, { status: 403 });
  }

  try {
    const users = await prisma.users.findMany({
      select: {
        id: true,
        username: true,
        email: true,
        role: true,
        approved: true,
        approved_at: true,
        created_at: true,
      },
      orderBy: [
        { approved: "asc" },
        { created_at: "desc" },
      ],
    });

    return NextResponse.json(users);
  } catch (error) {
    console.error("Error fetching users:", error);
    return NextResponse.json(
      { error: "Failed to fetch users" },
      { status: 500 }
    );
  }
}
