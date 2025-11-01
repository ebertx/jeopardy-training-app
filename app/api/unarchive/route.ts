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
    const { questionId } = await req.json();

    if (!questionId) {
      return NextResponse.json(
        { error: "Missing questionId" },
        { status: 400 }
      );
    }

    // Unarchive the question
    const updated = await prisma.jeopardy_questions.update({
      where: { id: questionId },
      data: {
        archived: false,
        archived_reason: null,
        archived_at: null,
      },
    });

    return NextResponse.json({
      success: true,
      message: "Question unarchived successfully",
      questionId: updated.id,
    });
  } catch (error) {
    console.error("Error unarchiving question:", error);
    return NextResponse.json(
      { error: "Failed to unarchive question" },
      { status: 500 }
    );
  }
}
