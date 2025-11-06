"use client";

import { useState, useEffect } from "react";
import { useSession } from "next-auth/react";
import { useRouter } from "next/navigation";
import { useParams } from "next/navigation";
import Navigation from "../../components/Navigation";

interface Question {
  col: number;
  row: number;
  question_id: number | null;
  value: number;
  answered: string | null;
  daily_double: boolean;
}

interface Round {
  categories: string[];
  questions: Question[];
}

interface GameBoard {
  rounds: {
    jeopardy: Round;
    double_jeopardy: Round;
    final_jeopardy: {
      category: string;
      question_id: number | null;
      answered: string | null;
    };
  };
}

interface GameState {
  id: number;
  gameBoard: GameBoard;
  jeopardy_score: number;
  double_j_score: number;
  final_score: number | null;
  current_round: number;
  questions_answered: number;
  completed_at: string | null;
}

interface QuestionData {
  id: number;
  question: string;
  answer: string;
  category: string;
}

export default function CoryatGamePage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const params = useParams();
  const gameId = params?.gameId as string;

  const [game, setGame] = useState<GameState | null>(null);
  const [loading, setLoading] = useState(true);
  const [currentRound, setCurrentRound] = useState<"jeopardy" | "double_jeopardy" | "final_jeopardy">("jeopardy");
  const [selectedQuestion, setSelectedQuestion] = useState<{ round: string; col: number; row: number } | null>(null);
  const [questionData, setQuestionData] = useState<QuestionData | null>(null);
  const [revealed, setRevealed] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [showRoundComplete, setShowRoundComplete] = useState(false);
  const [showGameComplete, setShowGameComplete] = useState(false);

  useEffect(() => {
    if (status === "unauthenticated") {
      router.push("/login");
    }
  }, [status, router]);

  useEffect(() => {
    if (status === "authenticated" && gameId) {
      fetchGame();
    }
  }, [status, gameId]);

  const fetchGame = async () => {
    try {
      const response = await fetch(`/api/coryat/${gameId}`);
      if (!response.ok) {
        router.push("/coryat");
        return;
      }
      const data = await response.json();
      setGame(data);

      // Determine current round based on answered questions
      const jeopardyQuestions = data.gameBoard.rounds.jeopardy.questions.filter((q: Question) => q.question_id !== null);
      const djQuestions = data.gameBoard.rounds.double_jeopardy.questions.filter((q: Question) => q.question_id !== null);
      const jeopardyAnswered = jeopardyQuestions.filter((q: Question) => q.answered !== null).length;
      const djAnswered = djQuestions.filter((q: Question) => q.answered !== null).length;

      if (jeopardyAnswered < jeopardyQuestions.length) {
        setCurrentRound("jeopardy");
      } else if (djAnswered < djQuestions.length) {
        setCurrentRound("double_jeopardy");
      } else if (!data.gameBoard.rounds.final_jeopardy.answered) {
        setCurrentRound("final_jeopardy");
      }

      setLoading(false);
    } catch (error) {
      console.error("Error fetching game:", error);
      setLoading(false);
    }
  };

  const handleClueClick = async (round: string, col: number, row: number) => {
    if (round === "final_jeopardy") {
      const fjQuestion = game!.gameBoard.rounds.final_jeopardy;
      if (!fjQuestion.question_id) return;

      setSelectedQuestion({ round, col, row });

      // Fetch question data from API
      try {
        const response = await fetch(`/api/questions/${fjQuestion.question_id}`);
        const data = await response.json();
        setQuestionData(data);
        setRevealed(false);
      } catch (error) {
        console.error("Error fetching question:", error);
      }
      return;
    }

    const roundKey = round as "jeopardy" | "double_jeopardy";
    const question = game!.gameBoard.rounds[roundKey].questions.find(
      (q) => q.col === col && q.row === row
    );

    if (!question || question.answered !== null || question.question_id === null) {
      return;
    }

    setSelectedQuestion({ round, col, row });

    // Fetch question data from API
    try {
      const response = await fetch(`/api/questions/${question.question_id}`);
      const data = await response.json();
      setQuestionData(data);
      setRevealed(false);
    } catch (error) {
      console.error("Error fetching question:", error);
    }
  };

  const handleAnswer = async (response: "correct" | "incorrect" | "pass") => {
    if (!selectedQuestion || submitting) return;

    setSubmitting(true);

    try {
      const res = await fetch(`/api/coryat/${gameId}/answer`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ...selectedQuestion, response })
      });

      if (res.ok) {
        await fetchGame();
        setSelectedQuestion(null);
        setQuestionData(null);
        setRevealed(false);

        // Check if round is complete
        const updatedGame = await res.json();
        if (updatedGame.questionsRemaining === 0) {
          setShowRoundComplete(true);
        }
      }
    } catch (error) {
      console.error("Error submitting answer:", error);
    } finally {
      setSubmitting(false);
    }
  };

  const handleCompleteGame = async () => {
    try {
      const response = await fetch(`/api/coryat/${gameId}/complete`, {
        method: "POST"
      });

      if (response.ok) {
        const data = await response.json();
        setShowGameComplete(true);
      }
    } catch (error) {
      console.error("Error completing game:", error);
    }
  };

  const renderBoard = (round: "jeopardy" | "double_jeopardy") => {
    const roundData = game!.gameBoard.rounds[round];
    const categories = roundData.categories;
    const questions = roundData.questions;

    // Group questions by column
    const columns: Question[][] = [];
    for (let col = 0; col < 6; col++) {
      columns.push(questions.filter(q => q.col === col).sort((a, b) => a.row - b.row));
    }

    return (
      <div className="overflow-x-auto">
        <div className="min-w-[800px] grid grid-cols-6 gap-2">
          {/* Category headers */}
          {categories.map((cat, i) => (
            <div key={i} className="bg-jeopardy-blue text-white p-3 rounded-t-lg text-center font-bold text-sm min-h-[60px] flex items-center justify-center">
              {cat}
            </div>
          ))}

          {/* Question cells */}
          {[0, 1, 2, 3, 4].map(row => (
            columns.map(col => {
              const question = col[row];
              const isAnswered = question.answered !== null;
              const isUnavailable = question.question_id === null;

              return (
                <button
                  key={`${question.col}-${question.row}`}
                  onClick={() => !isAnswered && !isUnavailable && handleClueClick(round, question.col, question.row)}
                  disabled={isAnswered || isUnavailable}
                  className={`
                    p-6 rounded-lg font-bold text-2xl min-h-[80px] flex items-center justify-center relative
                    ${isUnavailable ? 'bg-gray-300 text-gray-500 cursor-not-allowed' : ''}
                    ${isAnswered ? 'bg-gray-400 text-gray-600 cursor-not-allowed' : ''}
                    ${!isAnswered && !isUnavailable ? 'bg-jeopardy-blue text-jeopardy-gold hover:bg-blue-700 cursor-pointer' : ''}
                  `}
                >
                  {isUnavailable ? '—' : `$${question.value}`}
                  {question.daily_double && !isAnswered && (
                    <span className="absolute top-1 right-1 text-xs bg-red-600 text-white px-2 py-1 rounded">DD</span>
                  )}
                </button>
              );
            })
          ))}
        </div>
      </div>
    );
  };

  if (status === "loading" || loading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">Loading...</div>
      </div>
    );
  }

  if (!game) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">Game not found</div>
      </div>
    );
  }

  const roundTitle = currentRound === "jeopardy" ? "JEOPARDY!" : currentRound === "double_jeopardy" ? "DOUBLE JEOPARDY!" : "FINAL JEOPARDY!";
  const currentScore = currentRound === "jeopardy" ? game.jeopardy_score : game.jeopardy_score + game.double_j_score;

  return (
    <div className="min-h-screen bg-gray-100">
      <Navigation title="Coryat Mode" username={session?.user?.username} userRole={session?.user?.role} />

      {/* Score Bar */}
      <div className="bg-white shadow-sm p-4">
        <div className="max-w-6xl mx-auto flex justify-between items-center">
          <div>
            <span className="text-2xl font-bold text-jeopardy-blue">{roundTitle}</span>
          </div>
          <div className="text-right">
            <div className="text-sm text-gray-600">Current Score</div>
            <div className="text-2xl font-bold text-green-600">${currentScore.toLocaleString()}</div>
          </div>
        </div>
      </div>

      {/* Game Board */}
      <div className="max-w-6xl mx-auto p-8">
        {currentRound !== "final_jeopardy" ? (
          renderBoard(currentRound as "jeopardy" | "double_jeopardy")
        ) : (
          <div className="bg-white p-12 rounded-lg shadow-lg text-center">
            <h2 className="text-3xl font-bold text-jeopardy-blue mb-4">FINAL JEOPARDY!</h2>
            <p className="text-gray-600 mb-6">(For practice - not counted in Coryat score)</p>
            <div className="text-xl font-semibold mb-6">{game.gameBoard.rounds.final_jeopardy.category}</div>
            <button
              onClick={() => handleClueClick("final_jeopardy", 0, 0)}
              className="px-8 py-4 bg-jeopardy-blue text-white font-bold text-xl rounded-lg hover:bg-blue-700"
            >
              Reveal Clue
            </button>
          </div>
        )}
      </div>

      {/* Clue Modal */}
      {selectedQuestion && questionData && (
        <div className="fixed inset-0 bg-black bg-opacity-75 flex items-center justify-center z-50 p-4">
          <div className="bg-white rounded-lg max-w-2xl w-full p-8">
            <div className="mb-4 flex justify-between items-center">
              <span className="text-lg font-semibold">{questionData.category}</span>
              {selectedQuestion.round !== "final_jeopardy" && (
                <span className="text-2xl font-bold text-jeopardy-gold">
                  ${game.gameBoard.rounds[selectedQuestion.round as "jeopardy" | "double_jeopardy"].questions.find(q => q.col === selectedQuestion.col && q.row === selectedQuestion.row)?.value}
                </span>
              )}
            </div>

            <div className="bg-jeopardy-blue text-white p-6 rounded-lg mb-6 min-h-[150px] flex items-center justify-center">
              <div className="text-2xl text-center">{questionData.answer}</div>
            </div>

            {!revealed ? (
              <div className="text-center">
                <button
                  onClick={() => setRevealed(true)}
                  className="px-8 py-3 bg-jeopardy-gold text-jeopardy-blue font-bold text-lg rounded-lg hover:bg-yellow-400"
                >
                  Reveal Answer
                </button>
              </div>
            ) : (
              <>
                <div className="bg-green-50 p-4 rounded-lg mb-6">
                  <div className="text-sm text-gray-600 mb-1">Correct Response:</div>
                  <div className="text-xl font-bold text-green-800">{questionData.question}</div>
                </div>

                <div className="flex gap-3 justify-center">
                  <button
                    onClick={() => handleAnswer("incorrect")}
                    disabled={submitting}
                    className="px-6 py-3 bg-red-600 text-white font-bold rounded-lg hover:bg-red-700 disabled:opacity-50"
                  >
                    ✗ Incorrect
                  </button>
                  <button
                    onClick={() => handleAnswer("pass")}
                    disabled={submitting}
                    className="px-6 py-3 bg-gray-600 text-white font-bold rounded-lg hover:bg-gray-700 disabled:opacity-50"
                  >
                    — Pass
                  </button>
                  <button
                    onClick={() => handleAnswer("correct")}
                    disabled={submitting}
                    className="px-6 py-3 bg-green-600 text-white font-bold rounded-lg hover:bg-green-700 disabled:opacity-50"
                  >
                    ✓ Correct
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      )}

      {/* Round Complete Modal */}
      {showRoundComplete && (
        <div className="fixed inset-0 bg-black bg-opacity-75 flex items-center justify-center z-50">
          <div className="bg-white rounded-lg max-w-md w-full p-8 text-center">
            <h2 className="text-2xl font-bold mb-4">
              {currentRound === "jeopardy" ? "JEOPARDY! ROUND COMPLETE" : "DOUBLE JEOPARDY! COMPLETE"}
            </h2>
            <div className="text-3xl font-bold text-green-600 mb-6">
              ${currentRound === "jeopardy" ? game.jeopardy_score : game.double_j_score}
            </div>
            <button
              onClick={() => {
                setShowRoundComplete(false);
                if (currentRound === "jeopardy") {
                  setCurrentRound("double_jeopardy");
                } else {
                  setCurrentRound("final_jeopardy");
                }
              }}
              className="px-8 py-3 bg-jeopardy-blue text-white font-bold text-lg rounded-lg hover:bg-blue-700"
            >
              {currentRound === "jeopardy" ? "Continue to Double Jeopardy" : "Continue to Final Jeopardy"}
            </button>
          </div>
        </div>
      )}

      {/* Game Complete Modal */}
      {showGameComplete && (
        <div className="fixed inset-0 bg-black bg-opacity-75 flex items-center justify-center z-50">
          <div className="bg-white rounded-lg max-w-md w-full p-8 text-center">
            <h2 className="text-3xl font-bold text-jeopardy-blue mb-4">GAME COMPLETE!</h2>
            <div className="mb-6">
              <div className="text-sm text-gray-600">Your Coryat Score</div>
              <div className="text-4xl font-bold text-green-600 mb-4">
                ${(game.jeopardy_score + game.double_j_score).toLocaleString()}
              </div>
              <div className="text-sm space-y-1">
                <div>Jeopardy!: ${game.jeopardy_score.toLocaleString()}</div>
                <div>Double Jeopardy!: ${game.double_j_score.toLocaleString()}</div>
              </div>
            </div>
            <div className="flex gap-3">
              <button
                onClick={() => router.push("/coryat")}
                className="flex-1 px-6 py-3 bg-jeopardy-blue text-white font-bold rounded-lg hover:bg-blue-700"
              >
                Play Again
              </button>
              <button
                onClick={() => router.push("/coryat/history")}
                className="flex-1 px-6 py-3 bg-gray-600 text-white font-bold rounded-lg hover:bg-gray-700"
              >
                View History
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
