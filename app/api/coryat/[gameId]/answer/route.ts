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
    const { round, col, row, response } = await req.json();

    // Validate input
    if (!['jeopardy', 'double_jeopardy', 'final_jeopardy'].includes(round)) {
      return NextResponse.json({ error: "Invalid round" }, { status: 400 });
    }

    if (!['correct', 'incorrect', 'pass'].includes(response)) {
      return NextResponse.json({ error: "Invalid response" }, { status: 400 });
    }

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

    // Parse game board
    const gameBoard: any = game.game_board;
    let questionValue = 0;
    let scoreChange = 0;

    // Update the specific question
    if (round === 'final_jeopardy') {
      gameBoard.rounds.final_jeopardy.answered = response;
      // Final Jeopardy doesn't affect score per Coryat rules
    } else {
      // Find the question in the board
      const roundKey = round as 'jeopardy' | 'double_jeopardy';
      const questions = gameBoard.rounds[roundKey].questions;
      const questionIndex = questions.findIndex((q: any) => q.col === col && q.row === row);

      if (questionIndex === -1) {
        return NextResponse.json({ error: "Question not found" }, { status: 404 });
      }

      const question = questions[questionIndex];

      if (question.answered !== null) {
        return NextResponse.json({ error: "Question already answered" }, { status: 400 });
      }

      if (question.question_id === null) {
        return NextResponse.json({ error: "Question not available" }, { status: 400 });
      }

      // Update the question
      question.answered = response;
      questionValue = question.value;

      // Calculate score change
      if (response === 'correct') {
        scoreChange = questionValue;
      } else if (response === 'incorrect') {
        scoreChange = -questionValue;
      }
      // pass: scoreChange remains 0
    }

    // Update scores
    let newJeopardyScore = game.jeopardy_score;
    let newDoubleJScore = game.double_j_score;

    if (round === 'jeopardy') {
      newJeopardyScore += scoreChange;
    } else if (round === 'double_jeopardy') {
      newDoubleJScore += scoreChange;
    }

    // Update game in database
    const updatedGame = await prisma.coryat_games.update({
      where: { id: gameId },
      data: {
        game_board: gameBoard,
        jeopardy_score: newJeopardyScore,
        double_j_score: newDoubleJScore,
        questions_answered: game.questions_answered + 1
      }
    });

    // Calculate totals and remaining
    const totalScore = newJeopardyScore + newDoubleJScore;
    const jeopardyQuestions = gameBoard.rounds.jeopardy.questions.filter((q: any) => q.question_id !== null);
    const djQuestions = gameBoard.rounds.double_jeopardy.questions.filter((q: any) => q.question_id !== null);
    const totalQuestions = jeopardyQuestions.length + djQuestions.length;
    const questionsRemaining = totalQuestions - updatedGame.questions_answered;

    return NextResponse.json({
      success: true,
      scoreChange,
      currentRoundScore: round === 'jeopardy' ? newJeopardyScore : newDoubleJScore,
      totalScore,
      questionsRemaining,
      questionsAnswered: updatedGame.questions_answered
    });

  } catch (error) {
    console.error("Error submitting answer:", error);
    return NextResponse.json(
      { error: "Failed to submit answer" },
      { status: 500 }
    );
  }
}
