# Jeopardy! Training App

A web application for training and improving your Jeopardy! trivia skills using a database of 500,000+ classified questions.

## Features

### Core Features (MVP)
- **User Authentication**: Register and login with secure password hashing
- **Quiz Interface**: Interactive quiz with self-grading system
- **Performance Tracking**: Track your progress over time
- **Category Analytics**: View performance breakdown across 13 custom categories
- **Wrong Answer Review**: Review and learn from questions you got wrong
- **Category Filtering**: Filter questions by specific categories during quizzes

### Categories
The application includes 13 custom-classified categories:
- Art & Culture
- Business & Economics
- Film, TV & Pop Culture
- Geography & Exploration
- History & Politics
- Literature & Language
- Mathematics & Logic
- Miscellaneous
- Music & Performing Arts
- Philosophy, Religion & Society
- Science & Nature
- Sports & Games
- Technology & Engineering

## Tech Stack

- **Frontend/Backend**: Next.js 15 (React framework with App Router)
- **Database**: PostgreSQL with Prisma ORM
- **Authentication**: NextAuth.js with credentials provider
- **Styling**: Tailwind CSS
- **Charts**: Recharts
- **Password Hashing**: bcryptjs

## Getting Started

### Prerequisites
- Node.js 20+ installed
- Access to the PostgreSQL database (already configured)

### Installation

1. Install dependencies:
```bash
npm install
```

2. The database is already configured and tables have been created. The `.env` file contains the connection details.

3. Start the development server:
```bash
npm run dev
```

4. Open [http://localhost:3000](http://localhost:3000) in your browser

### First Time Setup

1. Visit the homepage
2. Click "Register" to create a new account
3. Fill in your username, email, and password
4. Login with your credentials
5. Start quizzing!

## Usage

### Quiz Mode
- Navigate to `/quiz` or click "Start Quiz" from the dashboard
- Use the category filter to focus on specific topics
- Read the question (clue) and think of your answer
- Press **Space** or click "Show Answer" to reveal the correct answer
- Self-grade by clicking "Correct ✓" or "Incorrect ✗"
- Or use keyboard shortcuts: **→** for correct, **←** for incorrect

### Keyboard Shortcuts
- **Space**: Reveal answer
- **→ (Right Arrow)**: Mark as correct
- **← (Left Arrow)**: Mark as incorrect

### Dashboard
- View overall statistics (total questions, correct answers, accuracy)
- See performance breakdown by category
- Identify strengths and weaknesses
- View color-coded category status:
  - **Green (Strong)**: 75%+ accuracy
  - **Yellow (Moderate)**: 50-74% accuracy
  - **Red (Needs Work)**: <50% accuracy

### Review Wrong Answers
- Navigate to `/review` or click "Review Wrong Answers"
- Filter by category to focus on specific areas
- Click on any question to expand and see the correct answer
- Use this to learn from your mistakes

## Database Schema

### Existing Table
- `jeopardy_questions`: 500,000+ Jeopardy questions with classifications

### New Tables (Created by the App)
- `users`: User accounts with authentication
- `quiz_sessions`: Quiz session tracking
- `question_attempts`: Individual question attempts with correct/incorrect marking

## Project Structure

```
jeopardy-training-app/
├── app/
│   ├── api/              # API routes
│   │   ├── auth/         # NextAuth authentication
│   │   ├── register/     # User registration
│   │   ├── quiz/         # Quiz endpoints
│   │   ├── stats/        # Statistics endpoint
│   │   ├── categories/   # Categories endpoint
│   │   └── review/       # Wrong answers endpoint
│   ├── quiz/            # Quiz page
│   ├── dashboard/       # Performance dashboard
│   ├── review/          # Wrong answers review
│   ├── login/           # Login page
│   ├── register/        # Registration page
│   ├── layout.tsx       # Root layout
│   └── page.tsx         # Landing page
├── components/          # Reusable components
├── lib/                # Utilities and database
│   ├── prisma.ts       # Prisma client singleton
│   └── auth.ts         # NextAuth configuration
├── prisma/
│   └── schema.prisma   # Database schema
└── types/              # TypeScript type definitions
```

## Future Enhancements

Potential features to add:
1. Study material recommendations based on weaknesses
2. Mobile app version (Flutter)
3. Advanced analytics and progress charts over time
4. Spaced repetition algorithm for optimal learning
5. Multiplayer/competitive modes
6. Daily challenges
7. Achievement system
8. Export progress reports

## Important Notes

- This application uses copyrighted Jeopardy! questions and cannot be released commercially
- The application is for personal training and educational purposes only
- Database credentials are stored in `.env` file (not committed to git)

## Development

### Build for Production
```bash
npm run build
npm start
```

### Linting
```bash
npm run lint
```

## License

This is a personal training application and is not licensed for commercial use due to the copyrighted nature of the Jeopardy! questions.
