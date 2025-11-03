"use client";

import { useState, useEffect } from "react";
import { useSession } from "next-auth/react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import Navigation from "../components/Navigation";

interface ArchivedQuestion {
  id: number;
  question: string;
  answer: string;
  category: string;
  classifier_category: string;
  archived_reason: string | null;
  archived_at: string | null;
  air_date: string | null;
}

export default function SettingsPage() {
  const { data: session, status } = useSession();
  const router = useRouter();
  const [archivedQuestions, setArchivedQuestions] = useState<ArchivedQuestion[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchTerm, setSearchTerm] = useState("");

  useEffect(() => {
    if (status === "unauthenticated") {
      router.push("/login");
    }
  }, [status, router]);

  useEffect(() => {
    if (status === "authenticated") {
      fetchArchivedQuestions();
    }
  }, [status]);

  const fetchArchivedQuestions = async () => {
    setLoading(true);
    try {
      const response = await fetch("/api/archive");
      const data = await response.json();
      setArchivedQuestions(data);
    } catch (error) {
      console.error("Error fetching archived questions:", error);
    } finally {
      setLoading(false);
    }
  };

  const handleUnarchive = async (questionId: number) => {
    if (!confirm("Unarchive this question? It will become available for all users again.")) {
      return;
    }

    try {
      await fetch("/api/unarchive", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ questionId }),
      });

      // Refresh the list
      fetchArchivedQuestions();
    } catch (error) {
      console.error("Error unarchiving question:", error);
    }
  };

  const filteredQuestions = archivedQuestions.filter(
    (q) =>
      q.answer.toLowerCase().includes(searchTerm.toLowerCase()) ||
      q.question.toLowerCase().includes(searchTerm.toLowerCase()) ||
      q.category.toLowerCase().includes(searchTerm.toLowerCase()) ||
      q.classifier_category?.toLowerCase().includes(searchTerm.toLowerCase())
  );

  if (status === "loading" || loading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-100">
        <div className="text-xl">Loading...</div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-100">
      <Navigation title="Settings & Archived Questions" username={session?.user?.username} userRole={session?.user?.role} />

      <div className="max-w-6xl mx-auto p-8">
        {/* Archived Questions Section */}
        <div className="bg-white p-6 rounded-lg shadow mb-8">
          <h2 className="text-2xl font-bold text-gray-800 mb-4">
            Archived Questions ({archivedQuestions.length})
          </h2>
          <p className="text-gray-600 mb-4">
            These questions have been archived (typically due to missing media) and are hidden from all users.
          </p>

          {/* Search */}
          <div className="mb-4">
            <input
              type="text"
              placeholder="Search archived questions..."
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              className="w-full px-4 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-jeopardy-blue"
            />
          </div>

          {filteredQuestions.length === 0 ? (
            <div className="text-center py-8 text-gray-500">
              {archivedQuestions.length === 0
                ? "No questions have been archived yet."
                : "No questions match your search."}
            </div>
          ) : (
            <div className="overflow-x-auto">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Clue
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Response
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Category
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Archived
                    </th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                      Action
                    </th>
                  </tr>
                </thead>
                <tbody className="bg-white divide-y divide-gray-200">
                  {filteredQuestions.map((q) => (
                    <tr key={q.id} className="hover:bg-gray-50">
                      <td className="px-6 py-4 text-sm text-gray-900 max-w-md">
                        <div className="line-clamp-2">{q.answer}</div>
                      </td>
                      <td className="px-6 py-4 text-sm text-gray-900 max-w-md">
                        <div className="line-clamp-2">{q.question}</div>
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                        <div className="font-medium">{q.classifier_category}</div>
                        <div className="text-xs text-gray-400">{q.category}</div>
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                        {q.archived_at && (
                          <div>
                            {new Date(q.archived_at).toLocaleDateString()}
                          </div>
                        )}
                        {q.archived_reason && (
                          <div className="text-xs text-gray-400">{q.archived_reason}</div>
                        )}
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap text-sm">
                        <button
                          onClick={() => handleUnarchive(q.id)}
                          className="px-4 py-2 bg-green-600 text-white font-semibold rounded hover:bg-green-700 transition-colors"
                        >
                          Unarchive
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
