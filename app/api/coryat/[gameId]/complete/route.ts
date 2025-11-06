import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

export async function POST(
  req: Request,
  { params }: { params: { gameId: string } }
) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const userId = parseInt(session.user.id);
  const gameId = parseInt(params.gameId);

  if (isNaN(gameId)) {
    return NextResponse.json({ error: "Invalid game ID" }, { status: 400 });
  }

  try {
    // Fetch the game
    const game = await prisma.coryat_games.findFirst({
      where: {
        id: gameId,
        user_id: userId
      }
    });

    if (!game) {
      return NextResponse.json({ error: "Game not found" }, { status: 404 });
    }

    if (game.completed_at) {
      return NextResponse.json({ error: "Game already completed" }, { status: 400 });
    }

    // Calculate final score (Jeopardy + Double Jeopardy only, per Coryat rules)
    const finalScore = game.jeopardy_score + game.double_j_score;

    // Calculate statistics
    const gameBoard: any = game.game_board;
    const jeopardyQuestions = gameBoard.rounds.jeopardy.questions;
    const djQuestions = gameBoard.rounds.double_jeopardy.questions;

    const allQuestions = [...jeopardyQuestions, ...djQuestions];
    const answered = allQuestions.filter((q: any) => q.answered !== null && q.question_id !== null);
    const correct = answered.filter((q: any) => q.answered === 'correct');
    const incorrect = answered.filter((q: any) => q.answered === 'incorrect');
    const passed = answered.filter((q: any) => q.answered === 'pass');

    const correctValue = correct.reduce((sum: number, q: any) => sum + q.value, 0);
    const incorrectValue = incorrect.reduce((sum: number, q: any) => sum + q.value, 0);

    // Mark game as completed
    const completedGame = await prisma.coryat_games.update({
      where: { id: gameId },
      data: {
        completed_at: new Date(),
        final_score: finalScore
      }
    });

    return NextResponse.json({
      success: true,
      summary: {
        final_score: finalScore,
        jeopardy_score: game.jeopardy_score,
        double_j_score: game.double_j_score,
        questions_answered: answered.length,
        correct: correct.length,
        incorrect: incorrect.length,
        passed: passed.length,
        correct_value: correctValue,
        incorrect_value: incorrectValue,
        completed_at: completedGame.completed_at
      }
    });

  } catch (error) {
    console.error("Error completing game:", error);
    return NextResponse.json(
      { error: "Failed to complete game" },
      { status: 500 }
    );
  }
}
