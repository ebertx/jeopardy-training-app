import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

export async function GET(req: Request) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const { searchParams } = new URL(req.url);
  const includeReviewed = searchParams.get("includeReviewed") === "true";
  const userId = parseInt(session.user.id);

  try {
    // Build base where clause for filtering sessions
    const sessionFilter = includeReviewed
      ? {} // Include all sessions
      : { is_review_session: false }; // Exclude review sessions by default

    // Overall stats
    const totalAttempts = await prisma.question_attempts.count({
      where: {
        user_id: userId,
        session: sessionFilter,
      },
    });

    const correctAttempts = await prisma.question_attempts.count({
      where: {
        user_id: userId,
        correct: true,
        session: sessionFilter,
      },
    });

    // Category breakdown
    const categoryStats = await prisma.question_attempts.groupBy({
      by: ["correct"],
      where: {
        user_id: userId,
        session: sessionFilter,
      },
      _count: {
        correct: true,
      },
    });

    // Get category-specific stats
    const categoryBreakdown = includeReviewed
      ? await prisma.$queryRaw<
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
          JOIN quiz_sessions qs ON qa.session_id = qs.id
          WHERE qa.user_id = ${userId}
            AND jq.archived = false
          GROUP BY jq.classifier_category
          ORDER BY jq.classifier_category
        `
      : await prisma.$queryRaw<
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
          JOIN quiz_sessions qs ON qa.session_id = qs.id
          WHERE qa.user_id = ${userId}
            AND jq.archived = false
            AND qs.is_review_session = false
          GROUP BY jq.classifier_category
          ORDER BY jq.classifier_category
        `;

    // Convert bigint to number for JSON serialization
    const formattedCategoryBreakdown = (categoryBreakdown || []).map((cat: any) => ({
      category: cat.classifier_category,
      total: Number(cat.total),
      correct: Number(cat.correct),
      accuracy: Number(cat.total) > 0
        ? Math.round((Number(cat.correct) / Number(cat.total)) * 100)
        : 0,
    }));

    // Recent sessions
    const recentSessions = await prisma.quiz_sessions.findMany({
      where: {
        user_id: userId,
        ...sessionFilter,
      },
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

    const formattedSessions = recentSessions.map((session: any) => ({
      id: session.id,
      started_at: session.started_at,
      completed_at: session.completed_at,
      total: session.question_attempts.length,
      correct: session.question_attempts.filter((a: any) => a.correct).length,
    }));

    // Get daily weighted average percentage correct
    // Uses weighted average: (total correct / total questions) * 100
    // This gives more weight to sessions with more questions
    const dailyStats = includeReviewed
      ? await prisma.$queryRaw<
          Array<{
            date: Date;
            avg_percentage: number;
            session_count: bigint;
          }>
        >`
          SELECT
            DATE(qs.completed_at) as date,
            CASE
              WHEN COUNT(qa.id) > 0
              THEN (SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END)::float / COUNT(qa.id)) * 100
              ELSE 0
            END::numeric(10,2) as avg_percentage,
            COUNT(DISTINCT qs.id)::bigint as session_count
          FROM quiz_sessions qs
          LEFT JOIN question_attempts qa ON qs.id = qa.session_id
          WHERE qs.user_id = ${userId}
            AND qs.completed_at IS NOT NULL
          GROUP BY DATE(qs.completed_at)
          ORDER BY date ASC
        `
      : await prisma.$queryRaw<
          Array<{
            date: Date;
            avg_percentage: number;
            session_count: bigint;
          }>
        >`
          SELECT
            DATE(qs.completed_at) as date,
            CASE
              WHEN COUNT(qa.id) > 0
              THEN (SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END)::float / COUNT(qa.id)) * 100
              ELSE 0
            END::numeric(10,2) as avg_percentage,
            COUNT(DISTINCT qs.id)::bigint as session_count
          FROM quiz_sessions qs
          LEFT JOIN question_attempts qa ON qs.id = qa.session_id
          WHERE qs.user_id = ${userId}
            AND qs.completed_at IS NOT NULL
            AND qs.is_review_session = false
          GROUP BY DATE(qs.completed_at)
          ORDER BY date ASC
        `;

    const formattedDailyStats = (dailyStats || []).map((day: any) => ({
      date: day.date.toISOString().split('T')[0],
      avgPercentage: parseFloat(day.avg_percentage) || 0,
      sessionCount: Number(day.session_count),
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
      dailyStats: formattedDailyStats,
    });
  } catch (error) {
    console.error("Error fetching stats:", error);
    console.error("Error details:", JSON.stringify(error, null, 2));
    return NextResponse.json(
      {
        error: "Failed to fetch statistics",
        details: error instanceof Error ? error.message : String(error)
      },
      { status: 500 }
    );
  }
}
