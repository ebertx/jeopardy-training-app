import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

export async function GET() {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const userId = parseInt(session.user.id);

  try {
    // Overall stats
    const totalAttempts = await prisma.question_attempts.count({
      where: { user_id: userId },
    });

    const correctAttempts = await prisma.question_attempts.count({
      where: { user_id: userId, correct: true },
    });

    // Category breakdown
    const categoryStats = await prisma.question_attempts.groupBy({
      by: ["correct"],
      where: {
        user_id: userId,
      },
      _count: {
        correct: true,
      },
    });

    // Get category-specific stats
    const categoryBreakdown = await prisma.$queryRaw<
      Array<{
        classifier_category: string;
        total: bigint;
        correct: bigint;
      }>
    >`
      SELECT
        jq.classifier_category,
        COUNT(*)::bigint as total,
        SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END)::bigint as correct
      FROM question_attempts qa
      JOIN jeopardy_questions jq ON qa.question_id = jq.id
      WHERE qa.user_id = ${userId}
        AND jq.archived = false
      GROUP BY jq.classifier_category
      ORDER BY jq.classifier_category
    `;

    // Convert bigint to number for JSON serialization
    const formattedCategoryBreakdown = categoryBreakdown.map((cat) => ({
      category: cat.classifier_category,
      total: Number(cat.total),
      correct: Number(cat.correct),
      accuracy: Number(cat.total) > 0
        ? Math.round((Number(cat.correct) / Number(cat.total)) * 100)
        : 0,
    }));

    // Recent sessions
    const recentSessions = await prisma.quiz_sessions.findMany({
      where: { user_id: userId },
      orderBy: { started_at: "desc" },
      take: 10,
      include: {
        question_attempts: {
          select: {
            correct: true,
          },
        },
      },
    });

    const formattedSessions = recentSessions.map((session) => ({
      id: session.id,
      started_at: session.started_at,
      completed_at: session.completed_at,
      total: session.question_attempts.length,
      correct: session.question_attempts.filter((a) => a.correct).length,
    }));

    return NextResponse.json({
      overall: {
        total: totalAttempts,
        correct: correctAttempts,
        accuracy: totalAttempts > 0
          ? Math.round((correctAttempts / totalAttempts) * 100)
          : 0,
      },
      categoryBreakdown: formattedCategoryBreakdown,
      recentSessions: formattedSessions,
    });
  } catch (error) {
    console.error("Error fetching stats:", error);
    return NextResponse.json(
      { error: "Failed to fetch statistics" },
      { status: 500 }
    );
  }
}
