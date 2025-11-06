import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

export async function GET(
  req: Request,
  { params }: { params: Promise<{ gameId: string }> }
) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const userId = parseInt(session.user.id);
  const { gameId: gameIdParam } = await params;
  const gameId = parseInt(gameIdParam);

  if (isNaN(gameId)) {
    return NextResponse.json({ error: "Invalid game ID" }, { status: 400 });
  }

  try {
    const game = await prisma.coryat_games.findFirst({
      where: {
        id: gameId,
        user_id: userId // Ensure user can only access their own games
      }
    });

    if (!game) {
      return NextResponse.json({ error: "Game not found" }, { status: 404 });
    }

    return NextResponse.json({
      id: game.id,
      started_at: game.started_at,
      completed_at: game.completed_at,
      gameBoard: game.game_board,
      jeopardy_score: game.jeopardy_score,
      double_j_score: game.double_j_score,
      final_score: game.final_score,
      current_round: game.current_round,
      questions_answered: game.questions_answered
    });

  } catch (error) {
    console.error("Error fetching Coryat game:", error);
    return NextResponse.json(
      { error: "Failed to fetch game" },
      { status: 500 }
    );
  }
}
