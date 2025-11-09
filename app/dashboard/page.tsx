"use client";

import { useState, useEffect } from "react";
import { useSession } from "next-auth/react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import Navigation from "../components/Navigation";
import {
  BarChart,
  Bar,
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
  Cell,
} from "recharts";

interface CategoryStat {
  category: string;
  total: number;
  correct: number;
  accuracy: number;
}

interface DailyStat {
  date: string;
  avgPercentage: number;
  sessionCount: number;
}

interface Stats {
  overall: {
    total: number;
    correct: number;
    accuracy: number;
  };
  categoryBreakdown: CategoryStat[];
  recentSessions: Array<{
    id: number;
    started_at: string;
    completed_at: string | null;
    total: number;
    correct: number;
  }>;
  dailyStats: DailyStat[];
}

export default function DashboardPage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const [stats, setStats] = useState<Stats | null>(null);
  const [loading, setLoading] = useState(true);
  const [includeReviewed, setIncludeReviewed] = useState(false);

  useEffect(() => {
    if (status === "unauthenticated") {
      router.push("/login");
    }
  }, [status, router]);

  useEffect(() => {
    if (status === "authenticated") {
      fetchStats();
    }
  }, [status, includeReviewed]);

  const fetchStats = async () => {
    try {
      const url = `/api/stats?includeReviewed=${includeReviewed}`;
      const response = await fetch(url);
      const data = await response.json();
      setStats(data);
    } catch (error) {
      console.error("Error fetching stats:", error);
    } finally {
      setLoading(false);
    }
  };

  if (status === "loading" || loading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">Loading...</div>
      </div>
    );
  }

  if (!stats) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">No data available</div>
      </div>
    );
  }

  // Get color based on accuracy/proficiency level
  const getAccuracyColor = (accuracy: number) => {
    if (accuracy >= 75) return "#10b981"; // green - Strong
    if (accuracy >= 50) return "#f59e0b"; // yellow/amber - Moderate
    return "#ef4444"; // red - Needs Work
  };

  // Add color to chart data
  const chartData = stats.categoryBreakdown.map(cat => ({
    ...cat,
    fill: getAccuracyColor(cat.accuracy)
  }));

  // Custom legend content for proficiency levels
  const renderLegend = () => (
    <div className="flex justify-center gap-6 mb-4">
      <div className="flex items-center gap-2">
        <div className="w-4 h-4 rounded" style={{ backgroundColor: "#10b981" }}></div>
        <span className="text-sm text-gray-600">Strong (≥75%)</span>
      </div>
      <div className="flex items-center gap-2">
        <div className="w-4 h-4 rounded" style={{ backgroundColor: "#f59e0b" }}></div>
        <span className="text-sm text-gray-600">Moderate (50-74%)</span>
      </div>
      <div className="flex items-center gap-2">
        <div className="w-4 h-4 rounded" style={{ backgroundColor: "#ef4444" }}></div>
        <span className="text-sm text-gray-600">Needs Work (&lt;50%)</span>
      </div>
    </div>
  );

  return (
    <div className="min-h-screen bg-gray-100">
      <Navigation title="Performance Dashboard" username={session?.user?.username} userRole={session?.user?.role} />

      <div className="max-w-6xl mx-auto p-8">
        {/* Filter Toggle */}
        <div className="bg-white p-4 rounded-lg shadow mb-6">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-sm font-medium text-gray-700 mb-1">Statistics Filter</h3>
              <p className="text-xs text-gray-500">
                {includeReviewed
                  ? "Showing all questions (including reviewed material)"
                  : "Showing only newly answered questions (review sessions excluded)"}
              </p>
            </div>
            <label className="flex items-center cursor-pointer">
              <div className="relative">
                <input
                  type="checkbox"
                  className="sr-only"
                  checked={includeReviewed}
                  onChange={(e) => setIncludeReviewed(e.target.checked)}
                />
                <div className={`block w-14 h-8 rounded-full transition-colors ${
                  includeReviewed ? 'bg-jeopardy-blue' : 'bg-gray-300'
                }`}></div>
                <div className={`dot absolute left-1 top-1 bg-white w-6 h-6 rounded-full transition-transform ${
                  includeReviewed ? 'transform translate-x-6' : ''
                }`}></div>
              </div>
              <div className="ml-3 text-sm font-medium text-gray-700">
                Include Review Sessions
              </div>
            </label>
          </div>
        </div>

        {/* Overall Stats */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
          <div className="bg-white p-6 rounded-lg shadow">
            <h3 className="text-gray-600 text-sm font-medium mb-2">Total Questions</h3>
            <p className="text-3xl font-bold text-jeopardy-blue">{stats.overall.total}</p>
          </div>
          <div className="bg-white p-6 rounded-lg shadow">
            <h3 className="text-gray-600 text-sm font-medium mb-2">Correct Answers</h3>
            <p className="text-3xl font-bold text-green-600">{stats.overall.correct}</p>
          </div>
          <div className="bg-white p-6 rounded-lg shadow">
            <h3 className="text-gray-600 text-sm font-medium mb-2">Overall Accuracy</h3>
            <p className="text-3xl font-bold text-jeopardy-blue">{stats.overall.accuracy}%</p>
          </div>
        </div>

        {/* Daily Session Performance */}
        {stats.dailyStats && stats.dailyStats.length > 0 && (
          <div className="bg-white p-6 rounded-lg shadow mb-8">
            <h2 className="text-xl font-bold text-gray-800 mb-4">
              Daily Session Performance
            </h2>
            <p className="text-sm text-gray-600 mb-4">
              Average percentage correct per day
            </p>
            <ResponsiveContainer width="100%" height={300}>
              <LineChart data={stats.dailyStats}>
                <CartesianGrid strokeDasharray="3 3" />
                <XAxis
                  dataKey="date"
                  angle={-45}
                  textAnchor="end"
                  height={80}
                  style={{ fontSize: "12px" }}
                />
                <YAxis
                  domain={[0, 100]}
                  label={{ value: 'Average % Correct', angle: -90, position: 'insideLeft' }}
                />
                <Tooltip
                  formatter={(value: number, name: string) => {
                    if (name === 'avgPercentage') {
                      return [`${value.toFixed(1)}%`, 'Avg % Correct'];
                    }
                    return [value, name];
                  }}
                  labelFormatter={(label: string) => `Date: ${label}`}
                />
                <Line
                  type="monotone"
                  dataKey="avgPercentage"
                  stroke="#0c47b7"
                  strokeWidth={2}
                  dot={{ fill: '#0c47b7', r: 4 }}
                  activeDot={{ r: 6 }}
                  name="Avg % Correct"
                />
              </LineChart>
            </ResponsiveContainer>
          </div>
        )}

        {/* Category Performance */}
        {stats.categoryBreakdown.length > 0 && (
          <div className="bg-white p-6 rounded-lg shadow mb-8">
            <h2 className="text-xl font-bold text-gray-800 mb-4">
              Performance by Category
            </h2>
            {renderLegend()}
            <ResponsiveContainer width="100%" height={400}>
              <BarChart data={chartData}>
                <CartesianGrid strokeDasharray="3 3" />
                <XAxis
                  dataKey="category"
                  angle={-45}
                  textAnchor="end"
                  height={150}
                  interval={0}
                  style={{ fontSize: "12px" }}
                />
                <YAxis domain={[0, 100]} label={{ value: 'Accuracy %', angle: -90, position: 'insideLeft' }} />
                <Tooltip />
                <Bar dataKey="accuracy" name="Accuracy %">
                  {chartData.map((entry, index) => (
                    <Cell key={`cell-${entry.category}`} fill={entry.fill} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
        )}

        {/* Category Breakdown Table */}
        {stats.categoryBreakdown.length > 0 && (
          <div className="bg-white p-6 rounded-lg shadow mb-8">
            <h2 className="text-xl font-bold text-gray-800 mb-4">
              Category Details
            </h2>
            <div className="overflow-x-auto">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Category
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Total
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Correct
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Accuracy
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Status
                    </th>
                  </tr>
                </thead>
                <tbody className="bg-white divide-y divide-gray-200">
                  {stats.categoryBreakdown
                    .sort((a, b) => a.accuracy - b.accuracy)
                    .map((cat) => (
                      <tr key={cat.category}>
                        <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                          {cat.category}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          {cat.total}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          {cat.correct}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          {cat.accuracy}%
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm">
                          {cat.accuracy >= 75 ? (
                            <span className="px-2 py-1 inline-flex text-xs leading-5 font-semibold rounded-full bg-green-100 text-green-800">
                              Strong
                            </span>
                          ) : cat.accuracy >= 50 ? (
                            <span className="px-2 py-1 inline-flex text-xs leading-5 font-semibold rounded-full bg-yellow-100 text-yellow-800">
                              Moderate
                            </span>
                          ) : (
                            <span className="px-2 py-1 inline-flex text-xs leading-5 font-semibold rounded-full bg-red-100 text-red-800">
                              Needs Work
                            </span>
                          )}
                        </td>
                      </tr>
                    ))}
                </tbody>
              </table>
            </div>
          </div>
        )}

        {/* Coryat Training */}
        <div className="bg-white p-6 rounded-lg shadow mb-8">
          <h2 className="text-xl font-bold text-gray-800 mb-4">
            Coryat Training Mode
          </h2>
          <p className="text-gray-600 mb-4">
            Practice with full Jeopardy! game boards using the Coryat scoring method.
            Track your progress and aim for the average contestant score of $24,000.
          </p>
          <Link
            href="/coryat"
            className="inline-block px-6 py-3 bg-jeopardy-gold text-jeopardy-blue font-bold rounded-lg hover:bg-yellow-400 transition-colors"
          >
            Start Coryat Game
          </Link>
        </div>

        {/* Action Buttons */}
        <div className="flex flex-col sm:flex-row sm:justify-center gap-4 items-stretch sm:items-center">
          <Link
            href="/quiz"
            className="px-8 py-4 bg-jeopardy-blue text-white font-bold text-xl rounded-lg hover:bg-blue-700 transition-colors text-center min-h-[60px] flex items-center justify-center"
          >
            Start Quiz
          </Link>
          {stats.overall.total > 0 && (
            <>
              <Link
                href="/review"
                className="px-8 py-4 bg-gray-600 text-white font-bold text-xl rounded-lg hover:bg-gray-700 transition-colors text-center min-h-[60px] flex items-center justify-center"
              >
                Review Wrong Answers
              </Link>
              <Link
                href="/mastered"
                className="px-8 py-4 bg-green-700 text-white font-bold text-xl rounded-lg hover:bg-green-800 transition-colors text-center min-h-[60px] flex items-center justify-center"
              >
                Review Mastered Questions
              </Link>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
