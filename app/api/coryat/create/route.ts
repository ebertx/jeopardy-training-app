import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

// When Jeopardy! doubled the dollar values
const DOUBLE_VALUE_DATE = new Date('2001-11-26');

// Normalize clue values for old episodes
function normalizeClueValue(value: number | null, airDate: Date | null): number | null {
  if (!value || !airDate) return value;

  if (airDate < DOUBLE_VALUE_DATE && value < 1000) {
    return value * 2;
  }
  return value;
}

// Select random categories with exponential weighting toward recent
async function selectCategories(count: number, excludeCategories: string[] = []): Promise<string[]> {
  // Get all categories with recent questions
  const categories = await prisma.jeopardy_questions.groupBy({
    by: ['category'],
    where: {
      category: {
        not: null,
        notIn: excludeCategories.length > 0 ? excludeCategories : undefined
      },
      archived: false,
      air_date: { not: null },
      classifier_category: { not: null }
    },
    _count: {
      category: true
    },
    orderBy: {
      _count: {
        category: 'desc'
      }
    },
    take: 100 // Get top 100 categories by question count
  });

  if (categories.length < count) {
    throw new Error(`Not enough categories available. Need ${count}, found ${categories.length}`);
  }

  // Shuffle and select
  const shuffled = categories
    .map((c: { category: string | null }) => c.category!)
    .sort(() => Math.random() - 0.5);

  return shuffled.slice(0, count);
}

// Find question for specific category and value
async function findQuestionForValue(
  category: string,
  targetValue: number,
  round: number
): Promise<any | null> {
  // Try exact match first (with proper value)
  let question = await prisma.jeopardy_questions.findFirst({
    where: {
      category,
      archived: false,
      clue_value: targetValue,
      OR: [
        { round },
        { round: null }
      ]
    },
    orderBy: {
      air_date: { sort: 'desc', nulls: 'last' }
    }
  });

  if (question) return question;

  // Try with NULL clue_value but matching round
  question = await prisma.jeopardy_questions.findFirst({
    where: {
      category,
      archived: false,
      round,
      clue_value: null
    },
    orderBy: {
      air_date: { sort: 'desc', nulls: 'last' }
    }
  });

  if (question) return question;

  // Try any question from the category with a clue_value that could be normalized
  question = await prisma.jeopardy_questions.findFirst({
    where: {
      category,
      archived: false,
      clue_value: { not: null }
    },
    orderBy: {
      air_date: { sort: 'desc', nulls: 'last' }
    }
  });

  if (question && question.clue_value && question.air_date) {
    const normalizedValue = normalizeClueValue(question.clue_value, question.air_date);
    if (normalizedValue === targetValue) {
      return question;
    }
  }

  // No suitable question found
  return null;
}

// Generate questions for one round
async function generateRoundQuestions(
  categories: string[],
  values: number[],
  round: number
) {
  const questions = [];

  for (let col = 0; col < categories.length; col++) {
    const category = categories[col];

    for (let row = 0; row < values.length; row++) {
      const value = values[row];
      const question = await findQuestionForValue(category, value, round);

      questions.push({
        col,
        row,
        question_id: question?.id || null,
        value,
        answered: null,
        daily_double: false
      });
    }
  }

  return questions;
}

// Assign Daily Doubles randomly
function assignDailyDoubles(questions: any[], count: number) {
  const availableIndices = questions
    .map((q, i) => ({ q, i }))
    .filter(({ q }) => q.question_id !== null) // Only assign to available questions
    .map(({ i }) => i);

  if (availableIndices.length === 0) return;

  const shuffled = availableIndices.sort(() => Math.random() - 0.5);
  const ddIndices = shuffled.slice(0, Math.min(count, shuffled.length));

  ddIndices.forEach(index => {
    questions[index].daily_double = true;
  });
}

// Find a Final Jeopardy question
async function findFinalJeopardyQuestion(): Promise<any | null> {
  // Try to find actual Final Jeopardy (round 3)
  let question = await prisma.jeopardy_questions.findFirst({
    where: {
      archived: false,
      round: 3,
    },
    orderBy: {
      air_date: { sort: 'desc', nulls: 'last' }
    }
  });

  if (question) return question;

  // Fall back to any recent question
  question = await prisma.jeopardy_questions.findFirst({
    where: {
      archived: false,
      question: { not: null },
      answer: { not: null },
    },
    orderBy: {
      air_date: { sort: 'desc', nulls: 'last' }
    }
  });

  return question;
}

export async function POST() {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const userId = parseInt(session.user.id);

  try {
    // Select 6 categories for Jeopardy Round
    const jeopardyCategories = await selectCategories(6);

    // Generate Jeopardy Round questions (values: 200, 400, 600, 800, 1000)
    const jeopardyQuestions = await generateRoundQuestions(
      jeopardyCategories,
      [200, 400, 600, 800, 1000],
      1
    );

    // Assign 1 Daily Double
    assignDailyDoubles(jeopardyQuestions, 1);

    // Select 6 different categories for Double Jeopardy
    const doubleJeopardyCategories = await selectCategories(6, jeopardyCategories);

    // Generate Double Jeopardy questions (values: 400, 800, 1200, 1600, 2000)
    const doubleJeopardyQuestions = await generateRoundQuestions(
      doubleJeopardyCategories,
      [400, 800, 1200, 1600, 2000],
      2
    );

    // Assign 2 Daily Doubles
    assignDailyDoubles(doubleJeopardyQuestions, 2);

    // Select Final Jeopardy question
    const finalJeopardyQuestion = await findFinalJeopardyQuestion();

    // Create game board structure
    const gameBoard = {
      rounds: {
        jeopardy: {
          categories: jeopardyCategories,
          questions: jeopardyQuestions
        },
        double_jeopardy: {
          categories: doubleJeopardyCategories,
          questions: doubleJeopardyQuestions
        },
        final_jeopardy: {
          category: finalJeopardyQuestion?.category || "FINAL JEOPARDY",
          question_id: finalJeopardyQuestion?.id || null,
          answered: null
        }
      }
    };

    // Create the game in the database
    const game = await prisma.coryat_games.create({
      data: {
        user_id: userId,
        game_board: gameBoard as any,
        current_round: 1,
        questions_answered: 0,
        jeopardy_score: 0,
        double_j_score: 0
      }
    });

    return NextResponse.json({
      success: true,
      gameId: game.id,
      gameBoard
    });

  } catch (error) {
    console.error("Error creating Coryat game:", error);
    return NextResponse.json(
      { error: "Failed to create Coryat game" },
      { status: 500 }
    );
  }
}
