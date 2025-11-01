"use client";

import { useState, useEffect } from "react";
import { useSession } from "next-auth/react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import Navigation from "../components/Navigation";

interface Question {
  id: number;
  question: string;
  answer: string;
  category: string;
  classifier_category: string;
  clue_value: number | null;
  round: number | null;
  air_date: string | null;
}

export default function QuizPage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const [question, setQuestion] = useState<Question | null>(null);
  const [prefetchedQuestion, setPrefetchedQuestion] = useState<Question | null>(null);
  const [showAnswer, setShowAnswer] = useState(false);
  const [loading, setLoading] = useState(true);
  const [sessionId, setSessionId] = useState<number | null>(null);
  const [categories, setCategories] = useState<Array<{ name: string; count: number }>>([]);
  const [selectedCategory, setSelectedCategory] = useState("all");
  const [stats, setStats] = useState({ total: 0, correct: 0 });

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

  // Prefetch next question when answer is revealed
  useEffect(() => {
    if (showAnswer && !prefetchedQuestion) {
      prefetchNextQuestion();
    }
  }, [showAnswer]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyPress = (e: KeyboardEvent) => {
      // Ignore if typing in an input
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) {
        return;
      }

      if (e.key === " " || e.key === "Spacebar") {
        e.preventDefault();
        if (!showAnswer) {
          setShowAnswer(true);
        }
      } else if (e.key === "ArrowLeft" && showAnswer) {
        e.preventDefault();
        handleAnswer(false);
      } else if (e.key === "ArrowRight" && showAnswer) {
        e.preventDefault();
        handleAnswer(true);
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

  const fetchQuestion = async (usePrefetch = true) => {
    // If we have a prefetched question and it's for the same category, use it
    if (usePrefetch && prefetchedQuestion && selectedCategory === selectedCategory) {
      setQuestion(prefetchedQuestion);
      setPrefetchedQuestion(null);
      setShowAnswer(false);
      setLoading(false);
      // Prefetch the next one in the background
      prefetchNextQuestion();
      return;
    }

    setLoading(true);
    setShowAnswer(false);

    try {
      const url = selectedCategory === "all"
        ? "/api/quiz/random"
        : `/api/quiz/random?category=${encodeURIComponent(selectedCategory)}`;

      const response = await fetch(url);
      const data = await response.json();
      setQuestion(data);
      setLoading(false);
      // Prefetch the next one in the background
      prefetchNextQuestion();
    } catch (error) {
      console.error("Error fetching question:", error);
      setLoading(false);
    }
  };

  const prefetchNextQuestion = async () => {
    try {
      const url = selectedCategory === "all"
        ? "/api/quiz/random"
        : `/api/quiz/random?category=${encodeURIComponent(selectedCategory)}`;

      const response = await fetch(url);
      const data = await response.json();
      setPrefetchedQuestion(data);
    } catch (error) {
      console.error("Error prefetching question:", error);
    }
  };

  const handleAnswer = async (correct: boolean) => {
    if (!question) return;

    // Update stats immediately for responsive UI
    setStats((prev) => ({
      total: prev.total + 1,
      correct: prev.correct + (correct ? 1 : 0),
    }));

    // If we have a prefetched question, show it immediately and submit in background
    if (prefetchedQuestion) {
      setQuestion(prefetchedQuestion);
      setPrefetchedQuestion(null);
      setShowAnswer(false);

      // Submit answer and prefetch next question in parallel
      Promise.all([
        fetch("/api/quiz/submit", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ questionId: question.id, correct, sessionId }),
        }).then(async (response) => {
          const data = await response.json();
          if (data.sessionId && !sessionId) {
            setSessionId(data.sessionId);
          }
        }),
        prefetchNextQuestion(),
      ]).catch((error) => {
        console.error("Error in background operations:", error);
      });
    } else {
      // No prefetched question - run submit and fetch in parallel
      try {
        const [submitResponse] = await Promise.all([
          fetch("/api/quiz/submit", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ questionId: question.id, correct, sessionId }),
          }),
          fetchQuestion(false), // Don't try to use prefetch since we know it's null
        ]);

        const data = await submitResponse.json();
        if (data.sessionId && !sessionId) {
          setSessionId(data.sessionId);
        }
      } catch (error) {
        console.error("Error submitting answer:", error);
      }
    }
  };

  const handleArchive = async () => {
    if (!question) return;

    if (!confirm("Archive this question? This will hide it for all users (useful for questions with missing media).")) {
      return;
    }

    try {
      // Archive and fetch next question in parallel
      await Promise.all([
        fetch("/api/archive", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            questionId: question.id,
            reason: "Missing media or unanswerable",
          }),
        }),
        fetchQuestion(true), // Use prefetch if available
      ]);
    } catch (error) {
      console.error("Error archiving question:", error);
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
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">No questions found</div>
      </div>
    );
  }

  const accuracy = stats.total > 0 ? Math.round((stats.correct / stats.total) * 100) : 0;

  return (
    <div className="min-h-screen bg-gray-100">
      <Navigation title="Jeopardy! Training" username={session?.user?.username} />

      {/* Session Stats & Category Filter */}
      <div className="bg-white shadow-sm p-4">
        <div className="max-w-6xl mx-auto">
          <div className="mb-4 text-center">
            <span className="text-sm font-medium text-gray-700">
              Session: {stats.correct}/{stats.total} ({accuracy}%)
            </span>
          </div>
          <label className="block text-sm font-medium text-gray-700 mb-2">
            Category Filter:
          </label>
          <select
            value={selectedCategory}
            onChange={(e) => setSelectedCategory(e.target.value)}
            className="px-4 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-jeopardy-blue"
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
        <div className="bg-jeopardy-blue text-jeopardy-gold p-8 rounded-lg shadow-xl">
          <div className="text-center mb-6">
            <div className="text-sm opacity-80 mb-2">
              {question.classifier_category}
              {question.clue_value && ` • $${question.clue_value}`}
            </div>
            <div className="text-3xl font-bold leading-relaxed">
              {question.answer}
            </div>
          </div>

          {!showAnswer ? (
            <div className="text-center mt-8">
              <button
                onClick={() => setShowAnswer(true)}
                className="px-8 py-3 bg-jeopardy-gold text-jeopardy-blue font-bold text-lg rounded-lg hover:bg-yellow-400 transition-colors"
              >
                Show Answer
              </button>
              <div className="text-white text-sm mt-4 opacity-70">
                Press Space to reveal
              </div>
            </div>
          ) : (
            <div className="mt-8">
              <div className="bg-white text-jeopardy-blue p-6 rounded-lg text-center mb-6">
                <div className="text-xl font-bold">{question.question}</div>
              </div>

              <div className="flex gap-4 justify-center flex-wrap">
                <button
                  onClick={() => handleAnswer(false)}
                  className="px-8 py-3 bg-red-600 text-white font-bold text-lg rounded-lg hover:bg-red-700 transition-colors"
                >
                  Incorrect ✗
                </button>
                <button
                  onClick={() => handleAnswer(true)}
                  className="px-8 py-3 bg-green-600 text-white font-bold text-lg rounded-lg hover:bg-green-700 transition-colors"
                >
                  Correct ✓
                </button>
              </div>

              <div className="text-white text-sm mt-4 text-center opacity-70">
                Press ← for Incorrect | → for Correct
              </div>

              <div className="mt-4 text-center">
                <button
                  onClick={handleArchive}
                  className="px-3 py-1 text-xs text-gray-300 hover:text-white underline transition-colors"
                >
                  Archive question (if media missing)
                </button>
              </div>
            </div>
          )}
        </div>

        {/* Additional Info */}
        <div className="mt-6 bg-white p-4 rounded-lg shadow-md">
          <div className="flex justify-center items-center gap-6 text-gray-800">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-jeopardy-blue">Original Category:</span>
              <span className="text-lg font-medium">{question.category}</span>
            </div>
            {question.air_date && (
              <>
                <span className="text-gray-400">•</span>
                <div className="flex items-center gap-2">
                  <span className="font-semibold text-jeopardy-blue">Aired:</span>
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
