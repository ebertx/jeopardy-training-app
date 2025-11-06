"use client";

import { useState, useEffect } from "react";
import { useSession } from "next-auth/react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import Navigation from "../../components/Navigation";

interface Game {
  id: number;
  completed_at: string;
  jeopardy_score: number;
  double_j_score: number;
  final_score: number;
  questions_answered: number;
}

interface Statistics {
  total_games: number;
  average_score: number;
  best_score: number;
  worst_score: number;
  trend: string;
}

export default function CoryatHistoryPage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const [games, setGames] = useState<Game[]>([]);
  const [statistics, setStatistics] = useState<Statistics | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (status === "unauthenticated") {
      router.push("/login");
    }
  }, [status, router]);

  useEffect(() => {
    if (status === "authenticated") {
      fetchHistory();
    }
  }, [status]);

  const fetchHistory = async () => {
    try {
      const response = await fetch("/api/coryat/history");
      const data = await response.json();
      setGames(data.games);
      setStatistics(data.statistics);
    } catch (error) {
      console.error("Error fetching history:", error);
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

  return (
    <div className="min-h-screen bg-gray-100">
      <Navigation title="Coryat History" username={session?.user?.username} userRole={session?.user?.role} />

      <div className="max-w-6xl mx-auto p-8">
        {/* Statistics Cards */}
        {statistics && statistics.total_games > 0 ? (
          <>
            <div className="grid grid-cols-1 md:grid-cols-4 gap-6 mb-8">
              <div className="bg-white p-6 rounded-lg shadow">
                <h3 className="text-gray-600 text-sm font-medium mb-2">Total Games</h3>
                <p className="text-3xl font-bold text-jeopardy-blue">{statistics.total_games}</p>
              </div>
              <div className="bg-white p-6 rounded-lg shadow">
                <h3 className="text-gray-600 text-sm font-medium mb-2">Average Score</h3>
                <p className="text-3xl font-bold text-green-600">${statistics.average_score.toLocaleString()}</p>
              </div>
              <div className="bg-white p-6 rounded-lg shadow">
                <h3 className="text-gray-600 text-sm font-medium mb-2">Best Score</h3>
                <p className="text-3xl font-bold text-jeopardy-gold">${statistics.best_score.toLocaleString()}</p>
              </div>
              <div className="bg-white p-6 rounded-lg shadow">
                <h3 className="text-gray-600 text-sm font-medium mb-2">Trend</h3>
                <p className={`text-3xl font-bold ${
                  statistics.trend === 'improving' ? 'text-green-600' :
                  statistics.trend === 'declining' ? 'text-red-600' :
                  'text-gray-600'
                }`}>
                  {statistics.trend === 'improving' ? '↗' :
                   statistics.trend === 'declining' ? '↘' : '→'}
                  <span className="text-base ml-2 capitalize">{statistics.trend}</span>
                </p>
              </div>
            </div>

            {/* Games List */}
            <div className="bg-white rounded-lg shadow overflow-hidden">
              <div className="p-6 border-b border-gray-200">
                <h2 className="text-2xl font-bold text-gray-800">Game History</h2>
              </div>
              <div className="overflow-x-auto">
                <table className="min-w-full divide-y divide-gray-200">
                  <thead className="bg-gray-50">
                    <tr>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                        Date
                      </th>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                        Final Score
                      </th>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                        Jeopardy!
                      </th>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                        Double J!
                      </th>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                        Questions
                      </th>
                    </tr>
                  </thead>
                  <tbody className="bg-white divide-y divide-gray-200">
                    {games.map((game) => (
                      <tr key={game.id} className="hover:bg-gray-50">
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-900">
                          {new Date(game.completed_at).toLocaleString()}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm font-bold text-green-600">
                          ${game.final_score.toLocaleString()}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          ${game.jeopardy_score.toLocaleString()}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          ${game.double_j_score.toLocaleString()}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          {game.questions_answered}/60
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>

            {/* Benchmarks Reference */}
            <div className="mt-8 bg-white p-6 rounded-lg shadow">
              <h3 className="font-semibold text-gray-800 mb-3">Coryat Score Benchmarks:</h3>
              <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm text-gray-700">
                <div className="text-center p-3 bg-red-50 rounded">
                  <div className="font-bold text-red-700">&lt;$16,000</div>
                  <div className="text-xs mt-1">Keep Practicing</div>
                </div>
                <div className="text-center p-3 bg-yellow-50 rounded">
                  <div className="font-bold text-yellow-700">~$24,000</div>
                  <div className="text-xs mt-1">Average Contestant</div>
                </div>
                <div className="text-center p-3 bg-green-50 rounded">
                  <div className="font-bold text-green-700">~$28,000</div>
                  <div className="text-xs mt-1">Strong Candidate</div>
                </div>
                <div className="text-center p-3 bg-blue-50 rounded">
                  <div className="font-bold text-blue-700">$32,000+</div>
                  <div className="text-xs mt-1">Excellent</div>
                </div>
              </div>
            </div>
          </>
        ) : (
          /* No Games Yet */
          <div className="bg-white p-12 rounded-lg shadow text-center">
            <h2 className="text-2xl font-bold text-gray-800 mb-4">
              No Games Played Yet
            </h2>
            <p className="text-gray-600 mb-6">
              Start your first Coryat training game to track your progress over time.
            </p>
            <Link
              href="/coryat"
              className="inline-block px-8 py-3 bg-jeopardy-blue text-white font-bold rounded-lg hover:bg-blue-700 transition-colors"
            >
              Start Your First Game
            </Link>
          </div>
        )}

        {/* Back Button */}
        <div className="mt-6 text-center">
          <Link
            href="/coryat"
            className="inline-block px-6 py-3 bg-gray-600 text-white font-semibold rounded-lg hover:bg-gray-700 transition-colors"
          >
            Back to Coryat Mode
          </Link>
        </div>
      </div>
    </div>
  );
}
