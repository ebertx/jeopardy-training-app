import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

export async function GET(
  req: Request,
  { params }: { params: Promise<{ questionId: string }> }
) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const { questionId: questionIdParam } = await params;
  const questionId = parseInt(questionIdParam);

  if (isNaN(questionId)) {
    return NextResponse.json({ error: "Invalid question ID" }, { status: 400 });
  }

  try {
    const question = await prisma.jeopardy_questions.findUnique({
      where: { id: questionId },
      select: {
        id: true,
        question: true,
        answer: true,
        category: true,
        clue_value: true,
        air_date: true
      }
    });

    if (!question) {
      return NextResponse.json({ error: "Question not found" }, { status: 404 });
    }

    return NextResponse.json(question);

  } catch (error) {
    console.error("Error fetching question:", error);
    return NextResponse.json(
      { error: "Failed to fetch question" },
      { status: 500 }
    );
  }
}
