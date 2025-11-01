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
    const userId = parseInt(session.user.id);

    if (!questionId) {
      return NextResponse.json(
        { error: "Missing questionId" },
        { status: 400 }
      );
    }

    // Reset mastery status
    const updated = await prisma.question_mastery.update({
      where: {
        user_id_question_id: {
          user_id: userId,
          question_id: questionId,
        },
      },
      data: {
        consecutive_correct: 0,
        mastered: false,
        mastered_at: null,
        last_attempt_at: new Date(),
      },
    });

    return NextResponse.json({
      success: true,
      message: "Mastery status reset successfully",
      questionId: updated.question_id,
    });
  } catch (error) {
    console.error("Error resetting mastery:", error);
    return NextResponse.json(
      { error: "Failed to reset mastery" },
      { status: 500 }
    );
  }
}
