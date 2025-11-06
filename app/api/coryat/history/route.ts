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
    // Get all completed games for the user
    const games = await prisma.coryat_games.findMany({
      where: {
        user_id: userId,
        completed_at: { not: null }
      },
      orderBy: {
        completed_at: 'desc'
      },
      select: {
        id: true,
        started_at: true,
        completed_at: true,
        jeopardy_score: true,
        double_j_score: true,
        final_score: true,
        questions_answered: true
      }
    });

    // Calculate statistics
    const totalGames = games.length;
    const scores = games.map(g => g.final_score || 0);
    const averageScore = totalGames > 0
      ? Math.round(scores.reduce((sum, score) => sum + score, 0) / totalGames)
      : 0;
    const bestScore = totalGames > 0 ? Math.max(...scores) : 0;
    const worstScore = totalGames > 0 ? Math.min(...scores) : 0;

    // Calculate trend (improving/declining/stable)
    let trend = 'stable';
    if (totalGames >= 3) {
      const recentGames = games.slice(0, 3);
      const olderGames = games.slice(-3);

      const recentAvg = recentGames.reduce((sum, g) => sum + (g.final_score || 0), 0) / recentGames.length;
      const olderAvg = olderGames.reduce((sum, g) => sum + (g.final_score || 0), 0) / olderGames.length;

      if (recentAvg > olderAvg * 1.1) {
        trend = 'improving';
      } else if (recentAvg < olderAvg * 0.9) {
        trend = 'declining';
      }
    }

    // Get recent incomplete game (if any)
    const incompleteGame = await prisma.coryat_games.findFirst({
      where: {
        user_id: userId,
        completed_at: null
      },
      orderBy: {
        started_at: 'desc'
      },
      select: {
        id: true,
        started_at: true,
        questions_answered: true,
        jeopardy_score: true,
        double_j_score: true
      }
    });

    return NextResponse.json({
      games,
      statistics: {
        total_games: totalGames,
        average_score: averageScore,
        best_score: bestScore,
        worst_score: worstScore,
        trend
      },
      incomplete_game: incompleteGame
    });

  } catch (error) {
    console.error("Error fetching Coryat history:", error);
    return NextResponse.json(
      { error: "Failed to fetch history" },
      { status: 500 }
    );
  }
}
