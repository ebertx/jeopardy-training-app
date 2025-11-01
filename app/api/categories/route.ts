import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

export async function GET() {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  try {
    const categories = await prisma.jeopardy_questions.groupBy({
      by: ["classifier_category"],
      where: {
        classifier_category: { not: null },
      },
      _count: {
        classifier_category: true,
      },
      orderBy: {
        classifier_category: "asc",
      },
    });

    const formattedCategories = categories.map((cat) => ({
      name: cat.classifier_category,
      count: cat._count.classifier_category,
    }));

    return NextResponse.json(formattedCategories);
  } catch (error) {
    console.error("Error fetching categories:", error);
    return NextResponse.json(
      { error: "Failed to fetch categories" },
      { status: 500 }
    );
  }
}
