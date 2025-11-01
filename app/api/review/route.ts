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
  const category = searchParams.get("category");
  const userId = parseInt(session.user.id);

  try {
    // Get distinct question IDs that have at least one incorrect attempt
    const incorrectQuestionIds = await prisma.question_attempts.findMany({
      where: {
        user_id: userId,
        correct: false,
      },
      distinct: ['question_id'],
      select: {
        question_id: true,
      },
    });

    const questionIds = incorrectQuestionIds.map(q => q.question_id);

    if (questionIds.length === 0) {
      return NextResponse.json([]);
    }

    // Get mastery records for these questions
    const masteryRecords = await prisma.question_mastery.findMany({
      where: {
        user_id: userId,
        question_id: { in: questionIds },
      },
    });

    // Filter out mastered questions
    const masteredQuestionIds = new Set(
      masteryRecords.filter(m => m.mastered).map(m => m.question_id)
    );

    const unmasteredQuestionIds = questionIds.filter(
      id => !masteredQuestionIds.has(id)
    );

    if (unmasteredQuestionIds.length === 0) {
      return NextResponse.json([]);
    }

    // Build category filter for questions
    const questionWhere: any = {
      id: { in: unmasteredQuestionIds },
      archived: false, // Exclude archived questions
    };

    if (category && category !== "all") {
      questionWhere.classifier_category = category;
    }

    // Get question details
    const questions = await prisma.jeopardy_questions.findMany({
      where: questionWhere,
      select: {
        id: true,
        question: true,
        answer: true,
        category: true,
        classifier_category: true,
        clue_value: true,
        round: true,
        air_date: true,
      },
    });

    // Create mastery lookup map
    const masteryMap = new Map(
      masteryRecords.map(m => [m.question_id, m])
    );

    // Format the response with mastery progress
    const formattedAnswers = questions.map((question) => {
      const mastery = masteryMap.get(question.id);
      return {
        question: {
          id: question.id,
          question: question.question,
          answer: question.answer,
          category: question.category,
          classifier_category: question.classifier_category,
          clue_value: question.clue_value,
          round: question.round,
          air_date: question.air_date,
        },
        masteryProgress: {
          consecutive_correct: mastery?.consecutive_correct || 0,
          required: 3,
        },
      };
    });

    // Sort by mastery progress (closest to mastery first)
    formattedAnswers.sort((a, b) => {
      return b.masteryProgress.consecutive_correct - a.masteryProgress.consecutive_correct;
    });

    return NextResponse.json(formattedAnswers);
  } catch (error) {
    console.error("Error fetching wrong answers:", error);
    return NextResponse.json(
      { error: "Failed to fetch wrong answers" },
      { status: 500 }
    );
  }
}
