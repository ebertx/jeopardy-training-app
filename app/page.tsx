import Link from "next/link";

export default function Home() {
  return (
    <div className="min-h-screen flex flex-col items-center justify-center bg-jeopardy-blue">
      <div className="text-center space-y-8 p-8">
        <h1 className="text-6xl font-bold text-jeopardy-gold mb-4">
          Jeopardy! Training
        </h1>
        <p className="text-2xl text-white mb-8">
          Master your trivia skills with 500,000+ questions
        </p>

        <div className="flex gap-4 justify-center">
          <Link
            href="/login"
            className="px-8 py-4 bg-jeopardy-gold text-jeopardy-blue font-bold text-xl rounded-lg hover:bg-yellow-400 transition-colors"
          >
            Login
          </Link>
          <Link
            href="/register"
            className="px-8 py-4 bg-white text-jeopardy-blue font-bold text-xl rounded-lg hover:bg-gray-100 transition-colors"
          >
            Register
          </Link>
        </div>

        <div className="mt-12 text-white text-sm">
          <p>Practice across 13 custom categories</p>
          <p>Track your progress and improve</p>
        </div>
      </div>
    </div>
  );
}
