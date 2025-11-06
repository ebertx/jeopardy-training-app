"use client";

import { useState, useEffect } from "react";
import { useSession } from "next-auth/react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import Navigation from "../components/Navigation";

interface IncompleteGame {
  id: number;
  started_at: string;
  questions_answered: number;
  jeopardy_score: number;
  double_j_score: number;
}

interface Statistics {
  total_games: number;
  average_score: number;
  best_score: number;
}

export default function CoryatLobbyPage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const [loading, setLoading] = useState(false);
  const [incompleteGame, setIncompleteGame] = useState<IncompleteGame | null>(null);
  const [statistics, setStatistics] = useState<Statistics | null>(null);

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
      setIncompleteGame(data.incomplete_game);
      setStatistics(data.statistics);
    } catch (error) {
      console.error("Error fetching history:", error);
    }
  };

  const handleStartNewGame = async () => {
    setLoading(true);
    try {
      const response = await fetch("/api/coryat/create", {
        method: "POST",
      });
      const data = await response.json();

      if (data.success) {
        router.push(`/coryat/${data.gameId}`);
      } else {
        alert("Failed to create game");
      }
    } catch (error) {
      console.error("Error creating game:", error);
      alert("Failed to create game");
    } finally {
      setLoading(false);
    }
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
      <Navigation title="Coryat Training Mode" username={session?.user?.username} userRole={session?.user?.role} />

      <div className="max-w-4xl mx-auto p-8">
        {/* Main Card */}
        <div className="bg-white p-8 rounded-lg shadow-lg mb-6">
          <h1 className="text-3xl font-bold text-jeopardy-blue mb-4">
            Coryat Training Mode
          </h1>
          <p className="text-gray-700 mb-6">
            Practice with full Jeopardy! game boards using the Coryat scoring method developed by Karl Coryat.
            Play through 60 questions across Jeopardy! and Double Jeopardy! rounds, plus Final Jeopardy! for practice.
          </p>

          {/* How It Works */}
          <div className="bg-blue-50 p-4 rounded-lg mb-6">
            <h3 className="font-semibold text-gray-800 mb-2">How It Works:</h3>
            <ul className="list-disc list-inside space-y-1 text-gray-700 text-sm">
              <li>Select clues from a 6×5 game board, just like the real show</li>
              <li>Add the dollar value for correct answers, subtract for incorrect</li>
              <li>Pass on questions you don't know (no penalty)</li>
              <li>Daily Doubles shown but scored as regular clues</li>
              <li>Final Jeopardy! for practice only (not counted in score)</li>
              <li>Your Coryat score = Jeopardy! + Double Jeopardy! totals</li>
            </ul>
          </div>

          {/* Statistics */}
          {statistics && statistics.total_games > 0 && (
            <div className="grid grid-cols-3 gap-4 mb-6">
              <div className="bg-gray-50 p-4 rounded-lg text-center">
                <div className="text-2xl font-bold text-jeopardy-blue">
                  {statistics.total_games}
                </div>
                <div className="text-sm text-gray-600">Games Played</div>
              </div>
              <div className="bg-gray-50 p-4 rounded-lg text-center">
                <div className="text-2xl font-bold text-green-600">
                  ${statistics.average_score.toLocaleString()}
                </div>
                <div className="text-sm text-gray-600">Average Score</div>
              </div>
              <div className="bg-gray-50 p-4 rounded-lg text-center">
                <div className="text-2xl font-bold text-jeopardy-gold">
                  ${statistics.best_score.toLocaleString()}
                </div>
                <div className="text-sm text-gray-600">Best Score</div>
              </div>
            </div>
          )}

          {/* Resume Incomplete Game */}
          {incompleteGame && (
            <div className="bg-yellow-50 border-2 border-yellow-300 p-4 rounded-lg mb-6">
              <h3 className="font-semibold text-gray-800 mb-2">Resume Game in Progress</h3>
              <p className="text-gray-700 mb-3">
                Started {new Date(incompleteGame.started_at).toLocaleString()} •
                {incompleteGame.questions_answered} questions answered •
                Current total: ${(incompleteGame.jeopardy_score + incompleteGame.double_j_score).toLocaleString()}
              </p>
              <Link
                href={`/coryat/${incompleteGame.id}`}
                className="inline-block px-6 py-3 bg-yellow-500 text-white font-bold rounded-lg hover:bg-yellow-600 transition-colors"
              >
                Resume Game
              </Link>
            </div>
          )}

          {/* Action Buttons */}
          <div className="flex gap-4">
            <button
              onClick={handleStartNewGame}
              disabled={loading}
              className="flex-1 px-8 py-4 bg-jeopardy-blue text-white font-bold text-xl rounded-lg hover:bg-blue-700 transition-colors disabled:bg-gray-400"
            >
              {loading ? "Creating Game..." : "Start New Game"}
            </button>
            <Link
              href="/coryat/history"
              className="px-8 py-4 bg-gray-600 text-white font-bold text-xl rounded-lg hover:bg-gray-700 transition-colors text-center"
            >
              View History
            </Link>
          </div>
        </div>

        {/* Scoring Reference */}
        <div className="bg-white p-6 rounded-lg shadow-lg">
          <h3 className="font-semibold text-gray-800 mb-3">Coryat Score Benchmarks:</h3>
          <div className="space-y-2 text-sm text-gray-700">
            <div className="flex justify-between">
              <span>Under $16,000:</span>
              <span className="font-medium">Keep practicing</span>
            </div>
            <div className="flex justify-between">
              <span>Around $24,000:</span>
              <span className="font-medium">Average contestant level</span>
            </div>
            <div className="flex justify-between">
              <span>Around $28,000:</span>
              <span className="font-medium">Strong audition candidate</span>
            </div>
            <div className="flex justify-between">
              <span>$32,000+:</span>
              <span className="font-medium">Excellent performance</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
