"use client";

import { ReactNode } from "react";

export interface QuestionCardProps {
  // Question data
  clue: string;
  answer: string;
  category: string;
  classifierCategory: string;
  clueValue: number | null;
  round: number | null;
  airDate: string | null;

  // Display state
  showAnswer: boolean;
  onRevealAnswer: () => void;

  // Answer handlers
  onCorrect: () => void;
  onIncorrect: () => void;

  // Optional elements
  badge?: ReactNode;
  additionalActions?: ReactNode;
  archiveButton?: ReactNode;
  keyboardHint?: string;

  // Styling customization
  cardBgColor?: string;
  cardTextColor?: string;
  buttonBgColor?: string;
  buttonTextColor?: string;
  buttonHoverColor?: string;

  // Loading states
  submitting?: boolean;
}

export default function QuestionCard({
  clue,
  answer,
  category,
  classifierCategory,
  clueValue,
  round,
  airDate,
  showAnswer: revealed,
  onRevealAnswer,
  onCorrect,
  onIncorrect,
  badge,
  additionalActions,
  archiveButton,
  keyboardHint,
  cardBgColor = "bg-jeopardy-blue",
  cardTextColor = "text-jeopardy-gold",
  buttonBgColor = "bg-jeopardy-gold",
  buttonTextColor = "text-jeopardy-blue",
  buttonHoverColor = "hover:bg-yellow-400",
  submitting = false,
}: QuestionCardProps) {
  // Determine if this is a Final Jeopardy question (round 3)
  const isFinalJeopardy = round === 3;
  return (
    <>
      {/* Question Card */}
      <div className={`${cardBgColor} ${cardTextColor} p-4 sm:p-6 md:p-8 rounded-lg shadow-xl`}>
        <div className="text-center mb-4 sm:mb-6">
          {/* Header with category, value, and optional badge */}
          <div className="text-xs sm:text-sm opacity-80 mb-2 flex justify-center items-center gap-2 flex-wrap">
            <span>
              {classifierCategory}
              {clueValue ? ` • $${clueValue}` : isFinalJeopardy ? " • Final Jeopardy" : ""}
            </span>
            {badge && <span>{badge}</span>}
          </div>

          {/* Clue/Question Display */}
          <div className="text-lg sm:text-xl md:text-2xl lg:text-3xl font-bold leading-relaxed break-words">
            {clue}
          </div>
        </div>

        {!revealed ? (
          /* Show Answer Button */
          <div className="text-center mt-6 sm:mt-8">
            <button
              onClick={onRevealAnswer}
              className={`px-6 py-2 sm:px-8 sm:py-3 ${buttonBgColor} ${buttonTextColor} font-bold text-base sm:text-lg rounded-lg ${buttonHoverColor} transition-colors`}
            >
              Show Answer
            </button>
            {keyboardHint && (
              <div className="text-white text-xs sm:text-sm mt-3 sm:mt-4 opacity-70">
                {keyboardHint}
              </div>
            )}
          </div>
        ) : (
          /* Answer Revealed */
          <div className="mt-6 sm:mt-8">
            {/* Answer Display */}
            <div className="bg-white text-jeopardy-blue p-4 sm:p-6 rounded-lg text-center mb-4 sm:mb-6">
              <div className="text-base sm:text-lg md:text-xl font-bold break-words">
                {answer}
              </div>
            </div>

            {/* Action Buttons */}
            <div className="flex gap-2 sm:gap-4 justify-center flex-wrap">
              <button
                onClick={onIncorrect}
                disabled={submitting}
                className="flex-1 min-w-[140px] sm:flex-none sm:px-6 md:px-8 py-2 sm:py-3 bg-red-600 text-white font-bold text-sm sm:text-base md:text-lg rounded-lg hover:bg-red-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                Incorrect ✗
              </button>
              <button
                onClick={onCorrect}
                disabled={submitting}
                className="flex-1 min-w-[140px] sm:flex-none sm:px-6 md:px-8 py-2 sm:py-3 bg-green-600 text-white font-bold text-sm sm:text-base md:text-lg rounded-lg hover:bg-green-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                Correct ✓
              </button>
              {additionalActions}
            </div>

            {keyboardHint && (
              <div className="text-white text-xs sm:text-sm mt-3 sm:mt-4 text-center opacity-70">
                {keyboardHint}
              </div>
            )}

            {archiveButton && (
              <div className="mt-3 sm:mt-4 text-center">
                {archiveButton}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Additional Info Footer */}
      <div className="mt-4 sm:mt-6 bg-white p-3 sm:p-4 rounded-lg shadow-md">
        <div className="flex flex-col sm:flex-row justify-center items-center gap-2 sm:gap-4 md:gap-6 text-gray-800 text-sm sm:text-base">
          <div className="flex items-center gap-2 flex-wrap justify-center">
            <span className="font-semibold text-jeopardy-blue text-xs sm:text-sm">
              Original Category:
            </span>
            <span className="text-sm sm:text-base md:text-lg font-medium break-words text-center">
              {category}
            </span>
          </div>
          {airDate && (
            <>
              <span className="text-gray-400 hidden sm:inline">•</span>
              <div className="flex items-center gap-2 flex-wrap justify-center">
                <span className="font-semibold text-jeopardy-blue text-xs sm:text-sm">
                  Aired:
                </span>
                <span className="text-sm sm:text-base md:text-lg font-medium">
                  {new Date(airDate).toLocaleDateString("en-US", {
                    year: "numeric",
                    month: "long",
                    day: "numeric",
                  })}
                </span>
              </div>
            </>
          )}
        </div>
      </div>
    </>
  );
}
