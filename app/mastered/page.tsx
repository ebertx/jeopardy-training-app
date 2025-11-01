"use client";

import { useState, useEffect } from "react";
import { useSession } from "next-auth/react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import Navigation from "../components/Navigation";

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
        <Navigation title="Mastered Questions Review" username={session?.user?.username} />

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
      <div className="max-w-4xl mx-auto p-8">
        <div className="bg-green-700 text-yellow-300 p-8 rounded-lg shadow-xl">
          <div className="text-center mb-6">
            <div className="flex justify-center items-center gap-3 text-sm opacity-90 mb-3">
              <span>{question.classifier_category}</span>
              {question.clue_value && <span>• ${question.clue_value}</span>}
              {question.mastered_at && (
                <span className="bg-yellow-300 text-green-800 px-3 py-1 rounded-full font-semibold text-xs">
                  Mastered {new Date(question.mastered_at).toLocaleDateString()}
                </span>
              )}
            </div>
            <div className="text-3xl font-bold leading-relaxed">
              {question.answer}
            </div>
          </div>

          {!showAnswer ? (
            <div className="text-center mt-8">
              <button
                onClick={() => setShowAnswer(true)}
                className="px-8 py-3 bg-yellow-300 text-green-800 font-bold text-lg rounded-lg hover:bg-yellow-200 transition-colors"
              >
                Show Answer
              </button>
              <div className="text-white text-sm mt-4 opacity-70">
                Press Space to reveal
              </div>
            </div>
          ) : (
            <div className="mt-8">
              <div className="bg-white text-green-800 p-6 rounded-lg text-center mb-6">
                <div className="text-xl font-bold">{question.question}</div>
              </div>

              <div className="flex gap-3 justify-center flex-wrap">
                <button
                  onClick={() => handleAnswer(false)}
                  className="px-6 py-3 bg-red-600 text-white font-bold text-lg rounded-lg hover:bg-red-700 transition-colors"
                >
                  Incorrect ✗
                </button>
                <button
                  onClick={() => handleAnswer(true)}
                  className="px-6 py-3 bg-green-600 text-white font-bold text-lg rounded-lg hover:bg-green-700 transition-colors"
                >
                  Correct ✓
                </button>
                <button
                  onClick={handleResetMastery}
                  className="px-6 py-3 bg-orange-600 text-white font-bold text-lg rounded-lg hover:bg-orange-700 transition-colors"
                >
                  🔄 Reset Mastery
                </button>
              </div>

              <div className="text-white text-sm mt-4 text-center opacity-70">
                Use "Reset Mastery" to add this question back to your review queue
              </div>
            </div>
          )}
        </div>

        {/* Additional Info */}
        <div className="mt-6 bg-white p-4 rounded-lg shadow-md">
          <div className="flex justify-center items-center gap-6 text-gray-800">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-green-700">Original Category:</span>
              <span className="text-lg font-medium">{question.category}</span>
            </div>
            {question.air_date && (
              <>
                <span className="text-gray-400">•</span>
                <div className="flex items-center gap-2">
                  <span className="font-semibold text-green-700">Aired:</span>
                  <span className="text-lg font-medium">
                    {new Date(question.air_date).toLocaleDateString("en-US", {
                      year: "numeric",
                      month: "long",
                      day: "numeric"
                    })}
                  </span>
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
