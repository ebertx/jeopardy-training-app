"use client";

import { useState, useEffect } from "react";
import { useSession } from "next-auth/react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import Navigation from "../components/Navigation";
import QuestionCard from "../components/QuestionCard";

interface MasteredQuestion {
  id: number;
  question: string;
  answer: string;
  category: string;
  classifier_category: string;
  clue_value: number | null;
  round: number | null;
  air_date: string | null;
  mastered_at: string | null;
  total_mastered: number;
}

export default function MasteredPage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const [question, setQuestion] = useState<MasteredQuestion | null>(null);
  const [showAnswer, setShowAnswer] = useState(false);
  const [loading, setLoading] = useState(true);
  const [categories, setCategories] = useState<Array<{ name: string; count: number }>>([]);
  const [selectedCategory, setSelectedCategory] = useState("all");

  useEffect(() => {
    if (status === "unauthenticated") {
      router.push("/login");
    }
  }, [status, router]);

  useEffect(() => {
    fetchCategories();
  }, []);

  useEffect(() => {
    if (status === "authenticated") {
      fetchQuestion();
    }
  }, [status, selectedCategory]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyPress = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) {
        return;
      }

      if (e.key === " " || e.key === "Spacebar") {
        e.preventDefault();
        if (!showAnswer) {
          setShowAnswer(true);
        }
      }
    };

    window.addEventListener("keydown", handleKeyPress);
    return () => window.removeEventListener("keydown", handleKeyPress);
  }, [showAnswer, question]);

  const fetchCategories = async () => {
    try {
      const response = await fetch("/api/categories");
      const data = await response.json();
      setCategories(data);
    } catch (error) {
      console.error("Error fetching categories:", error);
    }
  };

  const fetchQuestion = async () => {
    setLoading(true);
    setShowAnswer(false);

    try {
      const url = selectedCategory === "all"
        ? "/api/mastered"
        : `/api/mastered?category=${encodeURIComponent(selectedCategory)}`;

      const response = await fetch(url);

      if (!response.ok) {
        const error = await response.json();
        if (response.status === 404) {
          setQuestion(null);
        }
        throw new Error(error.error || "Failed to fetch question");
      }

      const data = await response.json();
      setQuestion(data);
    } catch (error) {
      console.error("Error fetching question:", error);
      setQuestion(null);
    } finally {
      setLoading(false);
    }
  };

  const handleAnswer = async (correct: boolean) => {
    if (!question) return;

    try {
      await fetch("/api/quiz/submit", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          questionId: question.id,
          correct,
          sessionId: null, // Don't track sessions for mastered review
          isReviewSession: true,
        }),
      });

      // Fetch next question
      setTimeout(() => {
        fetchQuestion();
      }, 300);
    } catch (error) {
      console.error("Error submitting answer:", error);
    }
  };

  const handleResetMastery = async () => {
    if (!question) return;

    if (!confirm("Are you sure you want to reset mastery for this question? It will be added back to your review queue.")) {
      return;
    }

    try {
      await fetch("/api/mastery/reset", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          questionId: question.id,
        }),
      });

      // Fetch next question
      setTimeout(() => {
        fetchQuestion();
      }, 300);
    } catch (error) {
      console.error("Error resetting mastery:", error);
    }
  };

  if (status === "loading" || loading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">Loading...</div>
      </div>
    );
  }

  if (!question) {
    return (
      <div className="min-h-screen bg-gray-100">
        <Navigation title="Mastered Questions Review" username={session?.user?.username} userRole={session?.user?.role} />

        <div className="max-w-4xl mx-auto p-8">
          <div className="bg-white p-12 rounded-lg shadow text-center">
            <h2 className="text-2xl font-bold text-gray-800 mb-4">
              No Mastered Questions Found
            </h2>
            <p className="text-gray-600 mb-6">
              {selectedCategory === "all"
                ? "You haven't mastered any questions yet. Answer questions correctly 3 times in a row to master them!"
                : `You haven't mastered any questions in the ${selectedCategory} category yet.`}
            </p>
            <Link
              href="/quiz"
              className="inline-block px-8 py-3 bg-jeopardy-blue text-white font-bold rounded-lg hover:bg-blue-700 transition-colors"
            >
              Start Quiz
            </Link>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-100">
      <Navigation title="Mastered Questions Review" username={session?.user?.username} bgColor="bg-green-700" />

      {/* Stats & Category Filter */}
      <div className="bg-white shadow-sm p-4">
        <div className="max-w-6xl mx-auto">
          <div className="mb-4 text-center">
            <span className="text-sm font-medium text-gray-700">
              {question.total_mastered} mastered question{question.total_mastered !== 1 ? "s" : ""}
            </span>
          </div>
          <label className="block text-sm font-medium text-gray-700 mb-2">
            Category Filter:
          </label>
          <select
            value={selectedCategory}
            onChange={(e) => setSelectedCategory(e.target.value)}
            className="px-4 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-green-700"
          >
            <option value="all">All Categories</option>
            {categories.map((cat) => (
              <option key={cat.name} value={cat.name}>
                {cat.name} ({cat.count})
              </option>
            ))}
          </select>
        </div>
      </div>

      {/* Question Card */}
      <div className="max-w-4xl mx-auto p-3 sm:p-4 md:p-6 lg:p-8">
        <QuestionCard
          clue={question.answer}
          answer={question.question}
          category={question.category}
          classifierCategory={question.classifier_category}
          clueValue={question.clue_value}
          airDate={question.air_date}
          showAnswer={showAnswer}
          onRevealAnswer={() => setShowAnswer(true)}
          onCorrect={() => handleAnswer(true)}
          onIncorrect={() => handleAnswer(false)}
          cardBgColor="bg-green-700"
          cardTextColor="text-yellow-300"
          buttonBgColor="bg-yellow-300"
          buttonTextColor="text-green-800"
          buttonHoverColor="hover:bg-yellow-200"
          keyboardHint={!showAnswer ? "Press Space to reveal" : undefined}
          badge={
            question.mastered_at ? (
              <span className="bg-yellow-300 text-green-800 px-2 sm:px-3 py-1 rounded-full font-semibold text-xs">
                Mastered {new Date(question.mastered_at).toLocaleDateString()}
              </span>
            ) : undefined
          }
          additionalActions={
            showAnswer ? (
              <button
                onClick={handleResetMastery}
                className="flex-1 min-w-[140px] sm:flex-none sm:px-6 md:px-8 py-2 sm:py-3 bg-orange-600 text-white font-bold text-sm sm:text-base md:text-lg rounded-lg hover:bg-orange-700 transition-colors"
              >
                🔄 Reset Mastery
              </button>
            ) : undefined
          }
        />
      </div>
    </div>
  );
}
