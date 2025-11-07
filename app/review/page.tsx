"use client";

import { useState, useEffect } from "react";
import { useSession } from "next-auth/react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import Navigation from "../components/Navigation";

interface WrongAnswer {
  question: {
    id: number;
    question: string;
    answer: string;
    category: string;
    classifier_category: string;
    clue_value: number | null;
    round: number | null;
    air_date: string | null;
  };
  masteryProgress: {
    consecutive_correct: number;
    required: number;
  };
}

export default function ReviewPage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const [wrongAnswers, setWrongAnswers] = useState<WrongAnswer[]>([]);
  const [loading, setLoading] = useState(true);
  const [categories, setCategories] = useState<Array<{ name: string; count: number }>>([]);
  const [selectedCategory, setSelectedCategory] = useState("all");
  const [expandedId, setExpandedId] = useState<number | null>(null);

  // Review session state
  const [inSession, setInSession] = useState(false);
  const [sessionQuestions, setSessionQuestions] = useState<WrongAnswer[]>([]);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [revealed, setRevealed] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (status === "unauthenticated") {
      router.push("/login");
    }
  }, [status, router]);

  useEffect(() => {
    if (status === "authenticated") {
      fetchCategories();
      fetchWrongAnswers();
    }
  }, [status, selectedCategory]);

  const fetchCategories = async () => {
    try {
      const response = await fetch("/api/categories");
      const data = await response.json();
      setCategories(data);
    } catch (error) {
      console.error("Error fetching categories:", error);
    }
  };

  const fetchWrongAnswers = async () => {
    setLoading(true);

    try {
      const url = selectedCategory === "all"
        ? "/api/review"
        : `/api/review?category=${encodeURIComponent(selectedCategory)}`;

      const response = await fetch(url);
      const data = await response.json();
      setWrongAnswers(data);
    } catch (error) {
      console.error("Error fetching wrong answers:", error);
    } finally {
      setLoading(false);
    }
  };

  const handleArchive = async (questionId: number) => {
    if (!confirm("Archive this question? This will hide it for all users (useful for questions with missing media).")) {
      return;
    }

    try {
      await fetch("/api/archive", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          questionId,
          reason: "Missing media or unanswerable",
        }),
      });

      // Refresh the list
      fetchWrongAnswers();
    } catch (error) {
      console.error("Error archiving question:", error);
    }
  };

  const startReviewSession = () => {
    setSessionQuestions([...wrongAnswers]);
    setCurrentIndex(0);
    setRevealed(false);
    setInSession(true);
  };

  const endReviewSession = () => {
    setInSession(false);
    setSessionQuestions([]);
    setCurrentIndex(0);
    setRevealed(false);
    fetchWrongAnswers(); // Refresh to show updated progress
  };

  const handleSessionSubmit = async (correct: boolean) => {
    if (submitting) return;

    setSubmitting(true);

    try {
      await fetch("/api/quiz/submit", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          questionId: sessionQuestions[currentIndex].question.id,
          correct,
          isReviewSession: true,
        }),
      });

      // Move to next question or end session
      if (currentIndex + 1 < sessionQuestions.length) {
        setCurrentIndex(currentIndex + 1);
        setRevealed(false);
      } else {
        // Session complete
        alert("Review session complete!");
        endReviewSession();
      }
    } catch (error) {
      console.error("Error submitting answer:", error);
      alert("Error submitting answer");
    } finally {
      setSubmitting(false);
    }
  };

  if (status === "loading" || loading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">Loading...</div>
      </div>
    );
  }

  // Review Session UI
  if (inSession && sessionQuestions.length > 0) {
    const currentQuestion = sessionQuestions[currentIndex].question;

    return (
      <div className="min-h-screen bg-gray-100">
        {/* Header */}
        <div className="bg-jeopardy-blue text-white p-3 sm:p-4">
          <div className="max-w-4xl mx-auto flex justify-between items-center gap-2">
            <h1 className="text-lg sm:text-xl md:text-2xl font-bold">Review Session</h1>
            <button
              onClick={endReviewSession}
              className="px-3 py-1.5 sm:px-4 sm:py-2 text-sm sm:text-base bg-white text-jeopardy-blue rounded hover:bg-gray-100 transition-colors whitespace-nowrap"
            >
              End Session
            </button>
          </div>
        </div>

        {/* Progress */}
        <div className="bg-white shadow-sm p-3 sm:p-4">
          <div className="max-w-4xl mx-auto">
            <div className="flex justify-between text-xs sm:text-sm text-gray-600">
              <span>Question {currentIndex + 1} of {sessionQuestions.length}</span>
              <span>{Math.round(((currentIndex) / sessionQuestions.length) * 100)}% Complete</span>
            </div>
            <div className="mt-2 w-full bg-gray-200 rounded-full h-2">
              <div
                className="bg-jeopardy-blue h-2 rounded-full transition-all"
                style={{ width: `${(currentIndex / sessionQuestions.length) * 100}%` }}
              ></div>
            </div>
          </div>
        </div>

        {/* Question Card */}
        <div className="max-w-4xl mx-auto p-3 sm:p-4 md:p-6 lg:p-8">
          <div className="bg-white rounded-lg shadow-lg p-4 sm:p-6 md:p-8">
            {/* Category Badge */}
            <div className="flex flex-wrap items-center gap-2 sm:gap-3 mb-4 sm:mb-6">
              <span className="px-2.5 py-1 sm:px-3 bg-blue-100 text-blue-800 text-xs sm:text-sm font-semibold rounded-full">
                {currentQuestion.classifier_category}
              </span>
              {currentQuestion.clue_value && (
                <span className="text-base sm:text-lg font-bold text-jeopardy-gold">
                  ${currentQuestion.clue_value}
                </span>
              )}
            </div>

            {/* Metadata */}
            <div className="bg-gray-50 p-3 sm:p-4 rounded-lg mb-4 sm:mb-6">
              <div className="flex flex-col sm:flex-row items-start sm:items-center gap-2 sm:gap-4 text-gray-700 text-sm sm:text-base">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="font-semibold text-jeopardy-blue text-xs sm:text-sm">Original Category:</span>
                  <span className="font-medium break-words">{currentQuestion.category}</span>
                </div>
                {currentQuestion.air_date && (
                  <>
                    <span className="text-gray-400 hidden sm:inline">•</span>
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="font-semibold text-jeopardy-blue text-xs sm:text-sm">Aired:</span>
                      <span className="font-medium">
                        {new Date(currentQuestion.air_date).toLocaleDateString("en-US", {
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

            {/* Clue */}
            <div className="mb-6 sm:mb-8 p-4 sm:p-6 bg-jeopardy-blue text-white rounded-lg">
              <div className="text-lg sm:text-xl md:text-2xl lg:text-3xl font-bold leading-relaxed break-words">
                {currentQuestion.answer}
              </div>
            </div>

            {/* Revealed Answer */}
            {revealed && (
              <div className="mb-4 sm:mb-6 p-4 sm:p-6 bg-green-50 rounded-lg border-2 border-green-200">
                <p className="text-xs sm:text-sm font-medium text-gray-700 mb-2">
                  Correct Response:
                </p>
                <div className="text-base sm:text-lg md:text-xl font-bold text-green-800 break-words">
                  {currentQuestion.question}
                </div>
              </div>
            )}

            {/* Action Buttons */}
            <div className="flex gap-2 sm:gap-4 justify-center flex-wrap">
              {!revealed ? (
                <button
                  onClick={() => setRevealed(true)}
                  className="px-6 py-3 sm:px-8 sm:py-4 bg-jeopardy-blue text-white text-base sm:text-lg md:text-xl font-bold rounded-lg hover:bg-blue-700 transition-colors"
                >
                  Reveal Answer
                </button>
              ) : (
                <>
                  <button
                    onClick={() => handleSessionSubmit(true)}
                    disabled={submitting}
                    className="flex-1 min-w-[140px] sm:flex-none sm:px-6 md:px-8 py-3 sm:py-4 bg-green-600 text-white text-base sm:text-lg md:text-xl font-bold rounded-lg hover:bg-green-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    ✓ Correct
                  </button>
                  <button
                    onClick={() => handleSessionSubmit(false)}
                    disabled={submitting}
                    className="flex-1 min-w-[140px] sm:flex-none sm:px-6 md:px-8 py-3 sm:py-4 bg-red-600 text-white text-base sm:text-lg md:text-xl font-bold rounded-lg hover:bg-red-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    ✗ Incorrect
                  </button>
                </>
              )}
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-100">
      <Navigation title="Review Wrong Answers" username={session?.user?.username} userRole={session?.user?.role} />

      {/* Category Filter */}
      <div className="bg-white shadow-sm p-3 sm:p-4">
        <div className="max-w-6xl mx-auto">
          <label className="block text-xs sm:text-sm font-medium text-gray-700 mb-2">
            Filter by Category:
          </label>
          <select
            value={selectedCategory}
            onChange={(e) => setSelectedCategory(e.target.value)}
            className="w-full sm:w-auto px-3 sm:px-4 py-2 text-sm sm:text-base border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-jeopardy-blue"
          >
            <option value="all">All Categories</option>
            {categories.map((cat) => (
              <option key={cat.name} value={cat.name}>
                {cat.name}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="max-w-6xl mx-auto p-3 sm:p-4 md:p-6 lg:p-8">
        {wrongAnswers.length === 0 ? (
          <div className="bg-white p-6 sm:p-8 md:p-12 rounded-lg shadow text-center">
            <h2 className="text-xl sm:text-2xl font-bold text-gray-800 mb-4">
              No Wrong Answers Found!
            </h2>
            <p className="text-sm sm:text-base text-gray-600 mb-6">
              {selectedCategory === "all"
                ? "You haven't answered any questions incorrectly yet, or you haven't started quizzing."
                : `You haven't answered any questions incorrectly in the ${selectedCategory} category.`}
            </p>
            <Link
              href="/quiz"
              className="inline-block px-6 py-2 sm:px-8 sm:py-3 text-sm sm:text-base bg-jeopardy-blue text-white font-bold rounded-lg hover:bg-blue-700 transition-colors"
            >
              Start Quiz
            </Link>
          </div>
        ) : (
          <>
            <div className="mb-4 sm:mb-6 flex flex-col sm:flex-row justify-between items-start sm:items-center gap-3 sm:gap-4">
              <h2 className="text-base sm:text-lg md:text-xl font-semibold text-gray-800">
                Found {wrongAnswers.length} question{wrongAnswers.length !== 1 ? "s" : ""} to review
              </h2>
              <button
                onClick={startReviewSession}
                className="w-full sm:w-auto px-4 sm:px-6 py-2 sm:py-3 text-sm sm:text-base bg-jeopardy-blue text-white font-bold rounded-lg hover:bg-blue-700 transition-colors whitespace-nowrap"
              >
                Start Review Session
              </button>
            </div>

            <div className="space-y-4">
              {wrongAnswers.map((item) => {
                const progress = item.masteryProgress.consecutive_correct;
                const required = item.masteryProgress.required;

                // Determine badge color based on progress
                let badgeColor = "bg-red-100 text-red-800"; // 0/3
                if (progress === 1) badgeColor = "bg-yellow-100 text-yellow-800"; // 1/3
                if (progress === 2) badgeColor = "bg-orange-100 text-orange-800"; // 2/3

                return (
                  <div
                    key={item.question.id}
                    className="bg-white rounded-lg shadow hover:shadow-md transition-shadow"
                  >
                    <div
                      className="p-4 sm:p-6 cursor-pointer"
                      onClick={() =>
                        setExpandedId(expandedId === item.question.id ? null : item.question.id)
                      }
                    >
                      <div className="flex justify-between items-start gap-2">
                        <div className="flex-1 min-w-0">
                          <div className="flex flex-wrap items-center gap-2 sm:gap-3 mb-2">
                            <span className="px-2 sm:px-3 py-1 bg-blue-100 text-blue-800 text-xs font-semibold rounded-full">
                              {item.question.classifier_category}
                            </span>
                            {item.question.clue_value && (
                              <span className="text-xs sm:text-sm text-gray-500">
                                ${item.question.clue_value}
                              </span>
                            )}
                            <span className={`px-2 sm:px-3 py-1 ${badgeColor} text-xs font-semibold rounded-full whitespace-nowrap`}>
                              Progress: {progress}/{required} ✓
                            </span>
                          </div>
                          <p className="text-sm sm:text-base md:text-lg font-medium text-gray-900 break-words">
                            {item.question.answer}
                          </p>
                        </div>
                        <button className="text-gray-400 hover:text-gray-600 text-xl flex-shrink-0 ml-2">
                          {expandedId === item.question.id ? "−" : "+"}
                        </button>
                      </div>

                      {expandedId === item.question.id && (
                      <div className="mt-3 sm:mt-4 pt-3 sm:pt-4 border-t border-gray-200">
                        <div className="bg-green-50 p-3 sm:p-4 rounded-lg mb-3 sm:mb-4">
                          <p className="text-xs sm:text-sm font-medium text-gray-700 mb-1">
                            Correct Response:
                          </p>
                          <p className="text-sm sm:text-base md:text-lg font-semibold text-green-800 break-words">
                            {item.question.question}
                          </p>
                        </div>
                        <div className="bg-gray-50 p-3 rounded-lg mt-3">
                          <div className="flex flex-col sm:flex-row items-start sm:items-center gap-2 sm:gap-4 text-gray-700 text-xs sm:text-sm">
                            <div className="flex items-center gap-2 flex-wrap">
                              <span className="font-semibold text-jeopardy-blue">Original Category:</span>
                              <span className="font-medium break-words">{item.question.category}</span>
                            </div>
                            {item.question.air_date && (
                              <>
                                <span className="text-gray-400 hidden sm:inline">•</span>
                                <div className="flex items-center gap-2 flex-wrap">
                                  <span className="font-semibold text-jeopardy-blue">Aired:</span>
                                  <span className="font-medium">
                                    {new Date(item.question.air_date).toLocaleDateString("en-US", {
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
                        <div className="mt-3 sm:mt-4 text-center">
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              handleArchive(item.question.id);
                            }}
                            className="px-3 py-1 text-xs text-gray-500 hover:text-gray-700 underline transition-colors"
                          >
                            Archive question (if media missing)
                          </button>
                        </div>
                        </div>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
