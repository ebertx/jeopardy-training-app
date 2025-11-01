import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";
import { openai } from "@/lib/openai";

interface StudyTopic {
  topic: string;
  explanation: string;
  readings: string[];
  wikipedia: string[];
  strategies: string[];
}

interface LLMResponse {
  analysis: string;
  topics: StudyTopic[];
}

export async function POST(req: Request) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  try {
    const { days } = await req.json();
    const userId = parseInt(session.user.id);

    if (!days || days <= 0) {
      return NextResponse.json(
        { error: "Invalid days parameter" },
        { status: 400 }
      );
    }

    // Calculate time period
    const now = new Date();
    const timePeriodStart = new Date(now.getTime() - days * 24 * 60 * 60 * 1000);

    // Get incorrect answers in the time period
    const incorrectAttempts = await prisma.question_attempts.findMany({
      where: {
        user_id: userId,
        correct: false,
        answered_at: {
          gte: timePeriodStart,
          lte: now,
        },
      },
      include: {
        question: {
          select: {
            question: true,
            answer: true,
            category: true,
            classifier_category: true,
          },
        },
      },
      orderBy: {
        answered_at: "desc",
      },
    });

    if (incorrectAttempts.length === 0) {
      return NextResponse.json(
        { error: "No incorrect answers found in the specified time period" },
        { status: 404 }
      );
    }

    // Group questions by classifier_category
    const categorizedQuestions: Record<string, Array<{ question: string; answer: string; category: string }>> = {};

    incorrectAttempts.forEach((attempt) => {
      const cat = attempt.question.classifier_category || "Uncategorized";
      if (!categorizedQuestions[cat]) {
        categorizedQuestions[cat] = [];
      }
      categorizedQuestions[cat].push({
        question: attempt.question.question || "",
        answer: attempt.question.answer || "",
        category: attempt.question.category || "",
      });
    });

    // Prepare prompt for OpenAI
    let questionsText = "";
    for (const [category, questions] of Object.entries(categorizedQuestions)) {
      questionsText += `\n## ${category} (${questions.length} questions)\n`;
      questions.slice(0, 10).forEach((q, i) => {
        questionsText += `${i + 1}. Clue: "${q.answer}"\n   Response: "${q.question}"\n   Original Category: ${q.category}\n`;
      });
      if (questions.length > 10) {
        questionsText += `   ... and ${questions.length - 10} more questions\n`;
      }
    }

    const prompt = `You are analyzing Jeopardy! quiz performance. The user answered ${incorrectAttempts.length} questions incorrectly in the past ${days} day(s).

Here are the incorrect questions, grouped by topic:
${questionsText}

Your task:
1. Analyze patterns across these incorrect answers to identify 3-5 core knowledge gaps
2. Group related topics together when possible (e.g., "Ancient Roman History + Greek Mythology" → "Classical Antiquity")
3. For each topic group, provide:
   - explanation: Brief description of the knowledge gap and why it matters
   - readings: Array of 2-3 specific book or article recommendations (with author names)
   - wikipedia: Array of 1-2 relevant Wikipedia article URLs (use actual URLs like https://en.wikipedia.org/wiki/...)
   - strategies: Array of study tips specific to learning this topic effectively

Return your response as JSON in this exact format:
{
  "analysis": "Overall pattern summary (2-3 sentences)",
  "topics": [
    {
      "topic": "Topic Name",
      "explanation": "Why this is a knowledge gap",
      "readings": ["Book/Article 1", "Book/Article 2"],
      "wikipedia": ["https://en.wikipedia.org/wiki/Topic1", "https://en.wikipedia.org/wiki/Topic2"],
      "strategies": ["Study tip 1", "Study tip 2"]
    }
  ]
}`;

    // Call OpenAI API
    const completion = await openai.chat.completions.create({
      model: "gpt-4o",
      messages: [
        {
          role: "system",
          content: "You are an expert educator helping students improve their Jeopardy! performance. Provide actionable, specific study recommendations.",
        },
        {
          role: "user",
          content: prompt,
        },
      ],
      response_format: { type: "json_object" },
      temperature: 0.7,
    });

    const responseText = completion.choices[0]?.message?.content;
    if (!responseText) {
      throw new Error("No response from OpenAI");
    }

    const llmResponse: LLMResponse = JSON.parse(responseText);

    // Save to database
    const recommendation = await prisma.study_recommendations.create({
      data: {
        user_id: userId,
        days_analyzed: days,
        analysis: llmResponse.analysis,
        recommendations: llmResponse.topics as any,
        question_count: incorrectAttempts.length,
        time_period_start: timePeriodStart,
        time_period_end: now,
      },
    });

    return NextResponse.json({
      success: true,
      recommendation: {
        id: recommendation.id,
        generated_at: recommendation.generated_at,
        days_analyzed: recommendation.days_analyzed,
        analysis: recommendation.analysis,
        topics: llmResponse.topics,
        question_count: recommendation.question_count,
        time_period_start: recommendation.time_period_start,
        time_period_end: recommendation.time_period_end,
      },
    });
  } catch (error) {
    console.error("Error generating study recommendations:", error);
    return NextResponse.json(
      { error: "Failed to generate study recommendations" },
      { status: 500 }
    );
  }
}
