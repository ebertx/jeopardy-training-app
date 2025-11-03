import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";
import { getOpenAI } from "@/lib/openai";

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

    incorrectAttempts.forEach((attempt: any) => {
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

    const prompt = `The user answered ${incorrectAttempts.length} Jeopardy! clues incorrectly in the past ${days} day(s).

Here are the missed clues, grouped by category:
${questionsText}

Return your response as JSON in this exact format:
{
  "analysis": "Overall pattern summary (2-3 sentences) - identify clue-level failure modes",
  "topics": [
    {
      "topic": "Memorable topic name (3-6 crisp umbrellas total)",
      "explanation": "Why this is a knowledge gap and Jeopardy!-specific patterns",
      "readings": ["Specific source 1 (must exist, high-yield)", "Specific source 2"],
      "wikipedia": ["https://en.wikipedia.org/wiki/CanonicalPage1"],
      "strategies": ["Concrete drill or mnemonic for Jeopardy! clue styles", "Buzzer/retrieval practice"]
    }
  ]
}`;

    // Call OpenAI API
    const completion = await getOpenAI().chat.completions.create({
      model: "gpt-4o",
      messages: [
        {
          role: "system",
          content: `You are a Jeopardy! training analyst channeling the wit and clarity of Ken Jennings. You produce error-free JSON only (no Markdown, no prose outside JSON). You transform the user's missed clues into a precise study plan with high-signal recommendations.

Constraints and behavior guidelines:
- Output must be valid JSON matching the provided schema exactly.
- Be concrete, specific, and Jeopardy!-aware: clue archetypes, wordplay, eponyms, before-&-after, homophones, hidden capitals, lateral hints, "pivot" facts that unlock families of clues.
- Group related topics into 3–6 crisp, memorable umbrellas (e.g., "Classical Antiquity" instead of many tiny slivers).
- Use a brisk, insightful, slightly playful tone reminiscent of Ken Jennings, but keep it practical and kind.
- Sources:
  - Prefer trustworthy, compact, high-yield texts: concise primers, annotated lists, museum/stanford/oxford resources, Library of Congress essays, Britannica, JSTOR open content, Project Gutenberg, open course handouts, reputable blogs by domain experts.
  - Only list sources that actually exist. If <90% confident, do not include it.
  - Provide at least one open/free source per topic if possible.
  - Wikipedia links: 1–2 canonical pages (no listicles unless the topic is itself a list).
- Study strategy:
  - Concrete drills (flash prompts, cloze deletions, mini-timelines, "ladder of facts" from obvious to obscure).
  - Mnemonics/memory hooks tuned to Jeopardy! clue styles.
  - Buzzer discipline and retrieval speed practices.
  - Cross-category bridges that turn isolated facts into networks.
- Pattern analysis:
  - Identify clue-level failure modes: misreading pivot words, ignoring dates/eras, missing wordplay, mixing adjacent domains, over-indexing on one exemplar, etc.
- Keep recommendations manageable: high-ROI only. Do not flood.

Validation:
- Return JSON that validates against the schema provided by the user.
- No extra keys. No comments. No trailing commas. No Markdown.`,
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
