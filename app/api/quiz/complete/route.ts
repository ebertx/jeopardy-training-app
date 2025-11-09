import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

export async function POST(req: Request) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  try {
    const { sessionId } = await req.json();
    const userId = parseInt(session.user.id);

    if (!sessionId) {
      return NextResponse.json(
        { error: "Session ID is required" },
        { status: 400 }
      );
    }

    // Verify the session belongs to the user
    const quizSession = await prisma.quiz_sessions.findFirst({
      where: {
        id: sessionId,
        user_id: userId,
      },
      include: {
        question_attempts: {
          select: {
            correct: true,
          },
        },
      },
    });

    if (!quizSession) {
      return NextResponse.json(
        { error: "Session not found" },
        { status: 404 }
      );
    }

    // Check if already completed
    if (quizSession.completed_at) {
      return NextResponse.json(
        { error: "Session already completed" },
        { status: 400 }
      );
    }

    // Mark session as completed
    const updatedSession = await prisma.quiz_sessions.update({
      where: { id: sessionId },
      data: {
        completed_at: new Date(),
      },
    });

    // Calculate statistics
    const totalQuestions = quizSession.question_attempts.length;
    const correctAnswers = quizSession.question_attempts.filter(
      (a: { correct: boolean }) => a.correct
    ).length;
    const accuracy =
      totalQuestions > 0
        ? Math.round((correctAnswers / totalQuestions) * 100)
        : 0;

    return NextResponse.json({
      success: true,
      summary: {
        total: totalQuestions,
        correct: correctAnswers,
        incorrect: totalQuestions - correctAnswers,
        accuracy,
        started_at: quizSession.started_at,
        completed_at: updatedSession.completed_at,
      },
    });
  } catch (error) {
    console.error("Error completing session:", error);
    return NextResponse.json(
      { error: "Failed to complete session" },
      { status: 500 }
    );
  }
}
