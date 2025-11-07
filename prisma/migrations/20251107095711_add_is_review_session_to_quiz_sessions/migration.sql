-- AlterTable
ALTER TABLE "quiz_sessions" ADD COLUMN "is_review_session" BOOLEAN NOT NULL DEFAULT false;

-- CreateIndex
CREATE INDEX "quiz_sessions_user_id_is_review_session_idx" ON "quiz_sessions"("user_id", "is_review_session");
