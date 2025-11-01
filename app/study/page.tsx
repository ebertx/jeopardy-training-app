"use client";

import { useState, useEffect } from "react";
import { useSession, signOut } from "next-auth/react";
import { useRouter } from "next/navigation";
import Link from "next/link";

interface StudyTopic {
  topic: string;
  explanation: string;
  readings: string[];
  wikipedia: string[];
  strategies: string[];
}

interface Recommendation {
  id: number;
  generated_at: string;
  days_analyzed: number;
  analysis: string;
  recommendations: StudyTopic[];
  question_count: number;
  time_period_start: string;
  time_period_end: string;
}

interface LatestInfo {
  generated_at: string;
  days_analyzed: number;
  question_count: number;
}

export default function StudyPage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const [days, setDays] = useState(7);
  const [loading, setLoading] = useState(false);
  const [latestRecommendation, setLatestRecommendation] = useState<Recommendation | null>(null);
  const [latestInfo, setLatestInfo] = useState<LatestInfo | null>(null);
  const [history, setHistory] = useState<Recommendation[]>([]);
  const [expandedHistory, setExpandedHistory] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (status === "unauthenticated") {
      router.push("/login");
    }
  }, [status, router]);

  useEffect(() => {
    if (status === "authenticated") {
      fetchLatestInfo();
      fetchHistory();
    }
  }, [status]);

  const fetchLatestInfo = async () => {
    try {
      const response = await fetch("/api/study/latest");
      const data = await response.json();
      setLatestInfo(data);
    } catch (error) {
      console.error("Error fetching latest info:", error);
    }
  };

  const fetchHistory = async () => {
    try {
      const response = await fetch("/api/study/history");
      const data = await response.json();

      if (data.length > 0) {
        // Parse recommendations if they're strings (from JSON field)
        const parsedData = data.map((item: any) => ({
          ...item,
          recommendations: typeof item.recommendations === 'string'
            ? JSON.parse(item.recommendations)
            : item.recommendations
        }));

        // Set the most recent as latest recommendation
        setLatestRecommendation(parsedData[0]);
        // Set the rest as history (excluding the first one)
        setHistory(parsedData.slice(1));
      }
    } catch (error) {
      console.error("Error fetching history:", error);
    }
  };

  const handleGenerate = async () => {
    setLoading(true);
    setError(null);

    try {
      const response = await fetch("/api/study/generate", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ days }),
      });

      if (!response.ok) {
        const errorData = await response.json();
        throw new Error(errorData.error || "Failed to generate recommendations");
      }

      const data = await response.json();

      // Parse recommendations if needed
      const recommendation = {
        ...data.recommendation,
        recommendations: typeof data.recommendation.recommendations === 'string'
          ? JSON.parse(data.recommendation.recommendations)
          : data.recommendation.recommendations
      };

      // Set the new recommendation as latest
      setLatestRecommendation(recommendation);

      // Move the old latest to history if it exists
      if (latestRecommendation) {
        setHistory([latestRecommendation, ...history]);
      }

      // Update latest info
      fetchLatestInfo();
    } catch (error: any) {
      console.error("Error generating recommendations:", error);
      setError(error.message);
    } finally {
      setLoading(false);
    }
  };

  const toggleHistoryExpand = (id: number) => {
    const newExpanded = new Set(expandedHistory);
    if (newExpanded.has(id)) {
      newExpanded.delete(id);
    } else {
      newExpanded.add(id);
    }
    setExpandedHistory(newExpanded);
  };

  if (status === "loading") {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">Loading...</div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-100">
      {/* Header */}
      <div className="bg-jeopardy-blue text-white p-4">
        <div className="max-w-6xl mx-auto flex justify-between items-center">
          <h1 className="text-2xl font-bold">Study Recommendations</h1>
          <div className="flex items-center gap-6">
            <span className="text-sm">Welcome, {session?.user?.username}!</span>
            <Link href="/quiz" className="hover:underline">
              Quiz
            </Link>
            <Link href="/review" className="hover:underline">
              Review
            </Link>
            <Link href="/mastered" className="hover:underline">
              Mastered
            </Link>
            <Link href="/dashboard" className="hover:underline">
              Dashboard
            </Link>
            <Link href="/settings" className="hover:underline">
              Settings
            </Link>
            <button
              onClick={() => signOut({ callbackUrl: "/" })}
              className="hover:underline text-sm"
            >
              Logout
            </button>
          </div>
        </div>
      </div>

      <div className="max-w-6xl mx-auto p-8">
        {/* Generation Form */}
        <div className="bg-white p-6 rounded-lg shadow mb-8">
          <h2 className="text-2xl font-bold text-gray-800 mb-4">
            Generate New Recommendations
          </h2>

          <div className="mb-4">
            <label className="block text-sm font-medium text-gray-700 mb-2">
              Analyze last
              <input
                type="number"
                min="1"
                max="365"
                value={days}
                onChange={(e) => setDays(parseInt(e.target.value) || 1)}
                className="mx-2 px-3 py-2 border border-gray-300 rounded-md w-20 text-center focus:outline-none focus:ring-2 focus:ring-jeopardy-blue"
              />
              days
            </label>

            {latestInfo && latestInfo.generated_at ? (
              <p className="text-sm text-gray-500 mt-2">
                Last generated: {new Date(latestInfo.generated_at).toLocaleString()} for {latestInfo.days_analyzed} days ({latestInfo.question_count} questions)
              </p>
            ) : (
              <p className="text-sm text-gray-500 mt-2">
                No recommendations generated yet
              </p>
            )}
          </div>

          <button
            onClick={handleGenerate}
            disabled={loading}
            className="px-6 py-3 bg-jeopardy-blue text-white font-bold rounded-lg hover:bg-blue-700 transition-colors disabled:bg-gray-400 disabled:cursor-not-allowed"
          >
            {loading ? (
              <span className="flex items-center gap-2">
                <svg className="animate-spin h-5 w-5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                </svg>
                Analyzing with AI...
              </span>
            ) : (
              "Generate Study Recommendations"
            )}
          </button>

          {error && (
            <div className="mt-4 p-4 bg-red-100 border border-red-400 text-red-700 rounded">
              {error}
            </div>
          )}
        </div>

        {/* Latest Recommendation */}
        {latestRecommendation && (
          <div className="bg-white p-6 rounded-lg shadow mb-8">
            <div className="mb-6">
              <h2 className="text-2xl font-bold text-gray-800 mb-2">
                Latest Recommendations
              </h2>
              <p className="text-sm text-gray-600">
                Generated {new Date(latestRecommendation.generated_at).toLocaleString()} •
                Analyzed {latestRecommendation.question_count} questions from {new Date(latestRecommendation.time_period_start).toLocaleDateString()} to {new Date(latestRecommendation.time_period_end).toLocaleDateString()}
              </p>
            </div>

            {/* Analysis Summary */}
            <div className="mb-6 p-4 bg-blue-50 rounded-lg">
              <h3 className="text-lg font-semibold text-gray-800 mb-2">
                Pattern Analysis
              </h3>
              <p className="text-gray-700">{latestRecommendation.analysis}</p>
            </div>

            {/* Topics */}
            <div className="space-y-6">
              {Array.isArray(latestRecommendation.recommendations) && latestRecommendation.recommendations.map((topic, index) => (
                <div key={index} className="border border-gray-200 rounded-lg p-6">
                  <h3 className="text-xl font-bold text-jeopardy-blue mb-3">
                    {topic.topic}
                  </h3>

                  <p className="text-gray-700 mb-4">{topic.explanation}</p>

                  {/* Reading Suggestions */}
                  <div className="mb-4">
                    <h4 className="font-semibold text-gray-800 mb-2">📚 Reading Suggestions:</h4>
                    <ul className="list-disc list-inside space-y-1">
                      {topic.readings.map((reading, i) => (
                        <li key={i} className="text-gray-700">{reading}</li>
                      ))}
                    </ul>
                  </div>

                  {/* Wikipedia Resources */}
                  {topic.wikipedia.length > 0 && (
                    <div className="mb-4">
                      <h4 className="font-semibold text-gray-800 mb-2">🔗 Wikipedia Resources:</h4>
                      <ul className="list-disc list-inside space-y-1">
                        {topic.wikipedia.map((link, i) => (
                          <li key={i}>
                            <a
                              href={link}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="text-blue-600 hover:underline"
                            >
                              {link.split('/wiki/')[1]?.replace(/_/g, ' ') || link}
                            </a>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}

                  {/* Study Strategies */}
                  <div>
                    <h4 className="font-semibold text-gray-800 mb-2">💡 Study Strategies:</h4>
                    <ul className="list-disc list-inside space-y-1">
                      {topic.strategies.map((strategy, i) => (
                        <li key={i} className="text-gray-700">{strategy}</li>
                      ))}
                    </ul>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Historical Recommendations */}
        {history.length > 0 && (
          <div className="bg-white p-6 rounded-lg shadow">
            <h2 className="text-2xl font-bold text-gray-800 mb-4">
              Historical Recommendations
            </h2>
            <p className="text-gray-600 mb-4">
              View past recommendations to track your study progress over time
            </p>

            <div className="space-y-4">
              {history.map((rec) => (
                <div key={rec.id} className="border border-gray-200 rounded-lg">
                  <div
                    className="p-4 cursor-pointer hover:bg-gray-50 transition-colors"
                    onClick={() => toggleHistoryExpand(rec.id)}
                  >
                    <div className="flex justify-between items-center">
                      <div>
                        <p className="font-semibold text-gray-800">
                          {new Date(rec.generated_at).toLocaleDateString()}
                        </p>
                        <p className="text-sm text-gray-600">
                          {rec.days_analyzed} days analyzed • {rec.question_count} questions • {Array.isArray(rec.recommendations) ? rec.recommendations.length : 0} topics
                        </p>
                      </div>
                      <button className="text-gray-400 hover:text-gray-600">
                        {expandedHistory.has(rec.id) ? "−" : "+"}
                      </button>
                    </div>
                  </div>

                  {expandedHistory.has(rec.id) && (
                    <div className="p-4 border-t border-gray-200 bg-gray-50">
                      <p className="text-gray-700 mb-4">{rec.analysis}</p>
                      <div className="space-y-3">
                        {Array.isArray(rec.recommendations) && rec.recommendations.map((topic, index) => (
                          <div key={index} className="text-sm">
                            <p className="font-semibold text-jeopardy-blue">{topic.topic}</p>
                            <p className="text-gray-600 text-xs">{topic.explanation}</p>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
