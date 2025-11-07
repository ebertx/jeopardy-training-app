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
    const { questionId, correct, sessionId, isReviewSession } = await req.json();
    const userId = parseInt(session.user.id);

    if (!questionId || typeof correct !== "boolean") {
      return NextResponse.json(
        { error: "Missing required fields" },
        { status: 400 }
      );
    }

    // If sessionId is provided, use it; otherwise create a new session
    let quizSessionId = sessionId;

    if (!quizSessionId) {
      const newSession = await prisma.quiz_sessions.create({
        data: {
          user_id: userId,
          started_at: new Date(),
          is_review_session: isReviewSession || false,
        },
      });
      quizSessionId = newSession.id;
    }

    // Record the attempt
    const attempt = await prisma.question_attempts.create({
      data: {
        session_id: quizSessionId,
        question_id: questionId,
        user_id: userId,
        correct,
        answered_at: new Date(),
      },
    });

    // Update or create mastery record
    const existingMastery = await prisma.question_mastery.findUnique({
      where: {
        user_id_question_id: {
          user_id: userId,
          question_id: questionId,
        },
      },
    });

    let masteryData;
    if (correct) {
      // Increment streak
      const newStreak = (existingMastery?.consecutive_correct || 0) + 1;
      const isMastered = newStreak >= 3;

      masteryData = {
        consecutive_correct: newStreak,
        mastered: isMastered,
        mastered_at: isMastered ? new Date() : existingMastery?.mastered_at,
        last_attempt_at: new Date(),
      };
    } else {
      // Reset streak on incorrect answer
      masteryData = {
        consecutive_correct: 0,
        mastered: false,
        mastered_at: null,
        last_attempt_at: new Date(),
      };
    }

    // Upsert mastery record
    await prisma.question_mastery.upsert({
      where: {
        user_id_question_id: {
          user_id: userId,
          question_id: questionId,
        },
      },
      update: masteryData,
      create: {
        user_id: userId,
        question_id: questionId,
        ...masteryData,
      },
    });

    return NextResponse.json({
      success: true,
      attemptId: attempt.id,
      sessionId: quizSessionId,
    });
  } catch (error) {
    console.error("Error submitting answer:", error);
    return NextResponse.json(
      { error: "Failed to submit answer" },
      { status: 500 }
    );
  }
}
