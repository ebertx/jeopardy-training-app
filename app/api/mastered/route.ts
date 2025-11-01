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
    // Get all mastered questions for the user
    const masteryRecords = await prisma.question_mastery.findMany({
      where: {
        user_id: userId,
        mastered: true,
        question: {
          archived: false, // Exclude archived questions
        },
      },
      include: {
        question: {
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
        },
      },
    });

    if (masteryRecords.length === 0) {
      return NextResponse.json({ error: "No mastered questions found" }, { status: 404 });
    }

    // Filter by category if specified
    let filteredRecords = masteryRecords;
    if (category && category !== "all") {
      filteredRecords = masteryRecords.filter(
        (record) => record.question.classifier_category === category
      );
    }

    if (filteredRecords.length === 0) {
      return NextResponse.json({ error: "No mastered questions found in this category" }, { status: 404 });
    }

    // Pick a random question
    const randomIndex = Math.floor(Math.random() * filteredRecords.length);
    const selectedRecord = filteredRecords[randomIndex];

    // Format response
    const response = {
      id: selectedRecord.question.id,
      question: selectedRecord.question.question,
      answer: selectedRecord.question.answer,
      category: selectedRecord.question.category,
      classifier_category: selectedRecord.question.classifier_category,
      clue_value: selectedRecord.question.clue_value,
      round: selectedRecord.question.round,
      air_date: selectedRecord.question.air_date,
      mastered_at: selectedRecord.mastered_at,
      total_mastered: filteredRecords.length,
    };

    return NextResponse.json(response);
  } catch (error) {
    console.error("Error fetching mastered questions:", error);
    return NextResponse.json(
      { error: "Failed to fetch mastered questions" },
      { status: 500 }
    );
  }
}
