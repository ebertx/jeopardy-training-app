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
  notes: string | null;
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
  const [gameTypeFilters, setGameTypeFilters] = useState<string[]>([]);
  const [loadingPreferences, setLoadingPreferences] = useState(true);
  const [showSessionSummary, setShowSessionSummary] = useState(false);
  const [sessionSummary, setSessionSummary] = useState<any>(null);

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
      loadPreferences();
    }
  }, [status]);

  useEffect(() => {
    if (status === "authenticated" && !loadingPreferences) {
      fetchQuestion();
    }
  }, [status, selectedCategory, gameTypeFilters, loadingPreferences]);

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

  const loadPreferences = async () => {
    try {
      const response = await fetch("/api/preferences");
      const data = await response.json();
      if (data.gameTypeFilters) {
        setGameTypeFilters(data.gameTypeFilters);
      }
    } catch (error) {
      console.error("Error loading preferences:", error);
    } finally {
      setLoadingPreferences(false);
    }
  };

  const savePreferences = async (filters: string[]) => {
    try {
      await fetch("/api/preferences", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ gameTypeFilters: filters }),
      });
    } catch (error) {
      console.error("Error saving preferences:", error);
    }
  };

  const buildQuizUrl = () => {
    const params = new URLSearchParams();
    if (selectedCategory !== "all") {
      params.append("category", selectedCategory);
    }
    if (gameTypeFilters.length > 0) {
      params.append("gameTypes", gameTypeFilters.join(","));
    }
    return `/api/quiz/random${params.toString() ? `?${params.toString()}` : ""}`;
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
      const url = buildQuizUrl();
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
      const url = buildQuizUrl();
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

  const handleEndSession = async () => {
    if (!sessionId) {
      alert("No active session to end");
      return;
    }

    if (!confirm("End this quiz session?")) {
      return;
    }

    try {
      const response = await fetch("/api/quiz/complete", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ sessionId }),
      });

      const data = await response.json();

      if (data.success) {
        setSessionSummary(data.summary);
        setShowSessionSummary(true);
        setSessionId(null);
        setStats({ total: 0, correct: 0 });
      } else {
        alert(data.error || "Failed to end session");
      }
    } catch (error) {
      console.error("Error ending session:", error);
      alert("Failed to end session");
    }
  };

  const handleGameTypeFilterChange = (type: string, checked: boolean) => {
    const newFilters = checked
      ? [...gameTypeFilters, type]
      : gameTypeFilters.filter((f) => f !== type);

    setGameTypeFilters(newFilters);
    savePreferences(newFilters);
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
      <Navigation title="Jeopardy! Training" username={session?.user?.username} userRole={session?.user?.role} />

      {/* Session Stats & Category Filter */}
      <div className="bg-white shadow-sm p-3 sm:p-4">
        <div className="max-w-6xl mx-auto">
          <div className="mb-3 sm:mb-4 flex flex-col sm:flex-row items-center justify-between gap-2">
            <div className="flex-1"></div>
            <span className="text-xs sm:text-sm font-medium text-gray-700">
              Session: {stats.correct}/{stats.total} ({accuracy}%)
            </span>
            <div className="flex-1 flex justify-end">
              {sessionId && stats.total > 0 && (
                <button
                  onClick={handleEndSession}
                  className="px-3 py-1 text-xs sm:text-sm bg-gray-600 text-white rounded hover:bg-gray-700 transition-colors"
                >
                  End Session
                </button>
              )}
            </div>
          </div>

          {/* Category Filter */}
          <div className="mb-3 sm:mb-4">
            <label className="block text-xs sm:text-sm font-medium text-gray-700 mb-2">
              Category Filter:
            </label>
            <select
              value={selectedCategory}
              onChange={(e) => setSelectedCategory(e.target.value)}
              className="w-full sm:w-auto px-3 sm:px-4 py-2 text-sm sm:text-base border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-jeopardy-blue"
            >
              <option value="all">All Categories</option>
              {categories.map((cat) => (
                <option key={cat.name} value={cat.name}>
                  {cat.name} ({cat.count})
                </option>
              ))}
            </select>
          </div>

          {/* Game Type Filter */}
          <div className="border-t border-gray-200 pt-3 sm:pt-4">
            <label className="block text-xs sm:text-sm font-medium text-gray-700 mb-2">
              Difficulty Level (Youth Mode):
            </label>
            <div className="flex flex-wrap gap-3 sm:gap-4">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={gameTypeFilters.includes("kids")}
                  onChange={(e) => handleGameTypeFilterChange("kids", e.target.checked)}
                  className="w-4 h-4 text-jeopardy-blue border-gray-300 rounded focus:ring-jeopardy-blue"
                />
                <span className="text-xs sm:text-sm text-gray-700">Kids Jeopardy</span>
              </label>
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={gameTypeFilters.includes("teen")}
                  onChange={(e) => handleGameTypeFilterChange("teen", e.target.checked)}
                  className="w-4 h-4 text-jeopardy-blue border-gray-300 rounded focus:ring-jeopardy-blue"
                />
                <span className="text-xs sm:text-sm text-gray-700">Teen Jeopardy</span>
              </label>
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={gameTypeFilters.includes("college")}
                  onChange={(e) => handleGameTypeFilterChange("college", e.target.checked)}
                  className="w-4 h-4 text-jeopardy-blue border-gray-300 rounded focus:ring-jeopardy-blue"
                />
                <span className="text-xs sm:text-sm text-gray-700">College Championship</span>
              </label>
              {gameTypeFilters.length > 0 && (
                <button
                  onClick={() => {
                    setGameTypeFilters([]);
                    savePreferences([]);
                  }}
                  className="text-xs text-gray-500 hover:text-gray-700 underline"
                >
                  Clear Filters
                </button>
              )}
            </div>
            {gameTypeFilters.length > 0 && (
              <p className="text-xs text-gray-500 mt-2 italic">
                Filter saved - will persist across sessions
              </p>
            )}
          </div>
        </div>
      </div>

      {/* Question Card */}
      <div className="max-w-4xl mx-auto p-3 sm:p-4 md:p-6 lg:p-8">
        <div className="bg-jeopardy-blue text-jeopardy-gold p-4 sm:p-6 md:p-8 rounded-lg shadow-xl">
          <div className="text-center mb-4 sm:mb-6">
            <div className="text-xs sm:text-sm opacity-80 mb-2">
              {question.classifier_category}
              {question.clue_value && ` • $${question.clue_value}`}
            </div>
            <div className="text-lg sm:text-xl md:text-2xl lg:text-3xl font-bold leading-relaxed">
              {question.answer}
            </div>
          </div>

          {!showAnswer ? (
            <div className="text-center mt-6 sm:mt-8">
              <button
                onClick={() => setShowAnswer(true)}
                className="px-6 py-2 sm:px-8 sm:py-3 bg-jeopardy-gold text-jeopardy-blue font-bold text-base sm:text-lg rounded-lg hover:bg-yellow-400 transition-colors"
              >
                Show Answer
              </button>
              <div className="text-white text-xs sm:text-sm mt-3 sm:mt-4 opacity-70">
                Press Space to reveal
              </div>
            </div>
          ) : (
            <div className="mt-6 sm:mt-8">
              <div className="bg-white text-jeopardy-blue p-4 sm:p-6 rounded-lg text-center mb-4 sm:mb-6">
                <div className="text-base sm:text-lg md:text-xl font-bold break-words">{question.question}</div>
              </div>

              <div className="flex gap-2 sm:gap-4 justify-center flex-wrap">
                <button
                  onClick={() => handleAnswer(false)}
                  className="flex-1 min-w-[140px] sm:flex-none sm:px-6 md:px-8 py-2 sm:py-3 bg-red-600 text-white font-bold text-sm sm:text-base md:text-lg rounded-lg hover:bg-red-700 transition-colors"
                >
                  Incorrect ✗
                </button>
                <button
                  onClick={() => handleAnswer(true)}
                  className="flex-1 min-w-[140px] sm:flex-none sm:px-6 md:px-8 py-2 sm:py-3 bg-green-600 text-white font-bold text-sm sm:text-base md:text-lg rounded-lg hover:bg-green-700 transition-colors"
                >
                  Correct ✓
                </button>
              </div>

              <div className="text-white text-xs sm:text-sm mt-3 sm:mt-4 text-center opacity-70">
                Press ← for Incorrect | → for Correct
              </div>

              <div className="mt-3 sm:mt-4 text-center">
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
        {/* Additional Info */}
        <div className="mt-4 sm:mt-6 bg-white p-3 sm:p-4 rounded-lg shadow-md">
          <div className="flex flex-col sm:flex-row justify-center items-center gap-2 sm:gap-4 md:gap-6 text-gray-800 text-sm sm:text-base">
            <div className="flex items-center gap-2 flex-wrap justify-center">
              <span className="font-semibold text-jeopardy-blue text-xs sm:text-sm">Original Category:</span>
              <span className="text-sm sm:text-base md:text-lg font-medium break-words text-center">{question.category}</span>
            </div>
            {question.air_date && (
              <>
                <span className="text-gray-400 hidden sm:inline">•</span>
                <div className="flex items-center gap-2 flex-wrap justify-center">
                  <span className="font-semibold text-jeopardy-blue text-xs sm:text-sm">Aired:</span>
                  <span className="text-sm sm:text-base md:text-lg font-medium">
                    {new Date(question.air_date).toLocaleDateString("en-US", {
                      year: "numeric",
                      month: "long",
                      day: "numeric"
                    })}
                  </span>
                </div>
              </>
            )}
            {question.notes && (
              <>
                <span className="text-gray-400">•</span>
                <div className="flex items-center gap-2">
                  <span className="text-sm text-gray-500 italic">{question.notes}</span>
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      {/* Session Summary Modal */}
      {showSessionSummary && sessionSummary && (
        <div className="fixed inset-0 bg-black bg-opacity-75 flex items-center justify-center z-50 p-4">
          <div className="bg-white rounded-lg max-w-md w-full p-6 sm:p-8">
            <h2 className="text-2xl font-bold text-jeopardy-blue mb-6 text-center">
              Session Complete!
            </h2>

            <div className="space-y-4 mb-6">
              <div className="bg-gray-50 p-4 rounded-lg">
                <div className="text-center">
                  <div className="text-4xl font-bold text-jeopardy-blue mb-2">
                    {sessionSummary.accuracy}%
                  </div>
                  <div className="text-sm text-gray-600">Accuracy</div>
                </div>
              </div>

              <div className="grid grid-cols-3 gap-3 text-center">
                <div className="bg-gray-50 p-3 rounded">
                  <div className="text-2xl font-bold text-gray-700">
                    {sessionSummary.total}
                  </div>
                  <div className="text-xs text-gray-500">Total</div>
                </div>
                <div className="bg-green-50 p-3 rounded">
                  <div className="text-2xl font-bold text-green-600">
                    {sessionSummary.correct}
                  </div>
                  <div className="text-xs text-gray-500">Correct</div>
                </div>
                <div className="bg-red-50 p-3 rounded">
                  <div className="text-2xl font-bold text-red-600">
                    {sessionSummary.incorrect}
                  </div>
                  <div className="text-xs text-gray-500">Incorrect</div>
                </div>
              </div>

              <div className="text-center text-sm text-gray-600">
                <div>
                  Started: {new Date(sessionSummary.started_at).toLocaleTimeString()}
                </div>
                <div>
                  Ended: {new Date(sessionSummary.completed_at).toLocaleTimeString()}
                </div>
              </div>
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => {
                  setShowSessionSummary(false);
                  router.push("/dashboard");
                }}
                className="flex-1 px-4 py-2 bg-gray-600 text-white font-semibold rounded-lg hover:bg-gray-700 transition-colors"
              >
                Go to Dashboard
              </button>
              <button
                onClick={() => setShowSessionSummary(false)}
                className="flex-1 px-4 py-2 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-700 transition-colors"
              >
                Start New Session
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
