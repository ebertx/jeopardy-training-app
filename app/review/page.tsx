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

  if (status === "loading" || loading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">Loading...</div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-100">
      <Navigation title="Review Wrong Answers" username={session?.user?.username} />

      {/* Category Filter */}
      <div className="bg-white shadow-sm p-4">
        <div className="max-w-6xl mx-auto">
          <label className="block text-sm font-medium text-gray-700 mb-2">
            Filter by Category:
          </label>
          <select
            value={selectedCategory}
            onChange={(e) => setSelectedCategory(e.target.value)}
            className="px-4 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-jeopardy-blue"
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

      <div className="max-w-6xl mx-auto p-8">
        {wrongAnswers.length === 0 ? (
          <div className="bg-white p-12 rounded-lg shadow text-center">
            <h2 className="text-2xl font-bold text-gray-800 mb-4">
              No Wrong Answers Found!
            </h2>
            <p className="text-gray-600 mb-6">
              {selectedCategory === "all"
                ? "You haven't answered any questions incorrectly yet, or you haven't started quizzing."
                : `You haven't answered any questions incorrectly in the ${selectedCategory} category.`}
            </p>
            <Link
              href="/quiz"
              className="inline-block px-8 py-3 bg-jeopardy-blue text-white font-bold rounded-lg hover:bg-blue-700 transition-colors"
            >
              Start Quiz
            </Link>
          </div>
        ) : (
          <>
            <div className="mb-6">
              <h2 className="text-xl font-semibold text-gray-800">
                Found {wrongAnswers.length} question{wrongAnswers.length !== 1 ? "s" : ""} to review
              </h2>
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
                      className="p-6 cursor-pointer"
                      onClick={() =>
                        setExpandedId(expandedId === item.question.id ? null : item.question.id)
                      }
                    >
                      <div className="flex justify-between items-start">
                        <div className="flex-1">
                          <div className="flex items-center gap-3 mb-2">
                            <span className="px-3 py-1 bg-blue-100 text-blue-800 text-xs font-semibold rounded-full">
                              {item.question.classifier_category}
                            </span>
                            {item.question.clue_value && (
                              <span className="text-sm text-gray-500">
                                ${item.question.clue_value}
                              </span>
                            )}
                            <span className={`px-3 py-1 ${badgeColor} text-xs font-semibold rounded-full`}>
                              Progress: {progress}/{required} ✓
                            </span>
                          </div>
                          <p className="text-lg font-medium text-gray-900">
                            {item.question.answer}
                          </p>
                        </div>
                        <button className="text-gray-400 hover:text-gray-600">
                          {expandedId === item.question.id ? "−" : "+"}
                        </button>
                      </div>

                      {expandedId === item.question.id && (
                      <div className="mt-4 pt-4 border-t border-gray-200">
                        <div className="bg-green-50 p-4 rounded-lg mb-4">
                          <p className="text-sm font-medium text-gray-700 mb-1">
                            Correct Response:
                          </p>
                          <p className="text-lg font-semibold text-green-800">
                            {item.question.question}
                          </p>
                        </div>
                        <div className="bg-gray-50 p-3 rounded-lg mt-3">
                          <div className="flex items-center gap-4 text-gray-700">
                            <div className="flex items-center gap-2">
                              <span className="font-semibold text-jeopardy-blue text-sm">Original Category:</span>
                              <span className="font-medium">{item.question.category}</span>
                            </div>
                            {item.question.air_date && (
                              <>
                                <span className="text-gray-400">•</span>
                                <div className="flex items-center gap-2">
                                  <span className="font-semibold text-jeopardy-blue text-sm">Aired:</span>
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
                        <div className="mt-4 text-center">
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
