import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

// Cache for question counts to avoid expensive COUNT queries
const countCache = new Map<string, { count: number; timestamp: number }>();
const CACHE_TTL = 5 * 60 * 1000; // 5 minutes

async function getCachedCount(where: any, cacheKey: string): Promise<number> {
  const cached = countCache.get(cacheKey);
  const now = Date.now();

  if (cached && now - cached.timestamp < CACHE_TTL) {
    return cached.count;
  }

  const count = await prisma.jeopardy_questions.count({ where });
  countCache.set(cacheKey, { count, timestamp: now });
  return count;
}

export async function GET(req: Request) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const { searchParams } = new URL(req.url);
  const category = searchParams.get("category");
  const gameTypesParam = searchParams.get("gameTypes");

  try {
    // Build where clause
    const where: any = {
      question: { not: null },
      answer: { not: null },
      classifier_category: { not: null },
      air_date: { not: null }, // Only include questions with air dates
      archived: false, // Exclude archived questions
    };

    if (category && category !== "all") {
      where.classifier_category = category;
    }

    // Add game type filter if provided
    if (gameTypesParam) {
      const gameTypes = gameTypesParam.split(",").filter(Boolean);

      if (gameTypes.length > 0) {
        // Build OR conditions for pattern matching
        const orConditions: any[] = [];

        gameTypes.forEach((type) => {
          if (type === "kids") {
            orConditions.push(
              { notes: { contains: "Kids", mode: "insensitive" } },
              { notes: { contains: "Kid's", mode: "insensitive" } }
            );
          } else if (type === "teen") {
            orConditions.push(
              { notes: { contains: "Teen", mode: "insensitive" } }
            );
          } else if (type === "college") {
            orConditions.push(
              { notes: { contains: "College", mode: "insensitive" } }
            );
          }
        });

        if (orConditions.length > 0) {
          where.OR = orConditions;
        }
      }
    }

    // Get total count with caching
    let cacheKey = category && category !== "all" ? `count_${category}` : "count_all";
    if (gameTypesParam) {
      cacheKey += `_${gameTypesParam}`;
    }
    const totalCount = await getCachedCount(where, cacheKey);

    if (totalCount === 0) {
      return NextResponse.json({ error: "No questions found" }, { status: 404 });
    }

    // Use exponential distribution to heavily favor recent questions
    // This will give ~70% probability to the most recent 20% of questions
    const lambda = 3.5; // Decay factor
    const randomValue = Math.random();
    const exponentialRandom = -Math.log(1 - randomValue) / lambda;
    const normalizedOffset = Math.min(exponentialRandom, 1); // Cap at 1
    const randomOffset = Math.floor(normalizedOffset * totalCount);

    // Fetch random question, ordered by air_date DESC (most recent first)
    const question = await prisma.jeopardy_questions.findFirst({
      where,
      orderBy: { air_date: "desc" },
      skip: randomOffset,
      select: {
        id: true,
        question: true,
        answer: true,
        category: true,
        classifier_category: true,
        clue_value: true,
        round: true,
        air_date: true,
        notes: true,
      },
    });

    return NextResponse.json(question);
  } catch (error) {
    console.error("Error fetching question:", error);
    return NextResponse.json(
      { error: "Failed to fetch question" },
      { status: 500 }
    );
  }
}
