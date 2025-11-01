import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

export async function GET() {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  try {
    const userId = parseInt(session.user.id);

    const latest = await prisma.study_recommendations.findFirst({
      where: {
        user_id: userId,
      },
      orderBy: {
        generated_at: "desc",
      },
      select: {
        generated_at: true,
        days_analyzed: true,
        question_count: true,
      },
    });

    return NextResponse.json(latest || null);
  } catch (error) {
    console.error("Error fetching latest study recommendation:", error);
    return NextResponse.json(
      { error: "Failed to fetch latest recommendation" },
      { status: 500 }
    );
  }
}
