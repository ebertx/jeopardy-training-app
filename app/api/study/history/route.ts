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

    const recommendations = await prisma.study_recommendations.findMany({
      where: {
        user_id: userId,
      },
      orderBy: {
        generated_at: "desc",
      },
    });

    return NextResponse.json(recommendations);
  } catch (error) {
    console.error("Error fetching study recommendation history:", error);
    return NextResponse.json(
      { error: "Failed to fetch recommendation history" },
      { status: 500 }
    );
  }
}
