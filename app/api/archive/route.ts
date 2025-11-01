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
    const { questionId, reason } = await req.json();

    if (!questionId) {
      return NextResponse.json(
        { error: "Missing questionId" },
        { status: 400 }
      );
    }

    // Archive the question
    const updated = await prisma.jeopardy_questions.update({
      where: { id: questionId },
      data: {
        archived: true,
        archived_reason: reason || "Missing media or unanswerable",
        archived_at: new Date(),
      },
    });

    return NextResponse.json({
      success: true,
      message: "Question archived successfully",
      questionId: updated.id,
    });
  } catch (error) {
    console.error("Error archiving question:", error);
    return NextResponse.json(
      { error: "Failed to archive question" },
      { status: 500 }
    );
  }
}

export async function GET(req: Request) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  try {
    // Get all archived questions
    const archivedQuestions = await prisma.jeopardy_questions.findMany({
      where: {
        archived: true,
      },
      orderBy: {
        archived_at: "desc",
      },
      select: {
        id: true,
        question: true,
        answer: true,
        category: true,
        classifier_category: true,
        archived_reason: true,
        archived_at: true,
        air_date: true,
      },
    });

    return NextResponse.json(archivedQuestions);
  } catch (error) {
    console.error("Error fetching archived questions:", error);
    return NextResponse.json(
      { error: "Failed to fetch archived questions" },
      { status: 500 }
    );
  }
}
