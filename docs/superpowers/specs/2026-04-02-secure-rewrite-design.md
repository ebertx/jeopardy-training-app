# Jeopardy Training App — Secure Rewrite Design

**Date:** 2026-04-02
**Status:** Approved
**Motivation:** The Next.js 15.5.6 deployment was compromised via a framework-level RCE. This rewrite eliminates the Node.js server attack surface entirely.

---

## Architecture Overview

The app is rewritten as a **Rust (Axum) monolith** serving a **Svelte 5 static SPA**. Deployment uses **Cloudflare Tunnel** — no ports are exposed on the host server.

```
Cloudflare Tunnel (WAF/TLS) → Docker (distroless) → Single Axum Binary
                                                      ├─ Static: Svelte 5 SPA
                                                      ├─ API: /api/*
                                                      ├─ Auth middleware (JWT)
                                                      └─ sqlx → PostgreSQL (existing)
```

### Security layers (outside in)

1. **Cloudflare WAF** — bot protection, rate limiting, IP reputation
2. **Cloudflare Tunnel** — no inbound ports on Tower
3. **Distroless container** — no shell, no tools, no attack surface
4. **Rust binary** — memory safe, no runtime, compiled dependencies
5. **sqlx compile-time checked queries** — SQL injection not possible
6. **Argon2 password hashing** — replaces bcrypt
7. **JWT in HttpOnly/Secure/SameSite=Strict cookies** — no localStorage tokens
8. **Security headers** — CSP, X-Frame-Options, etc. via tower-http

---

## Backend Structure

Single Rust crate, module-based organization.

### Directory layout

```
src/
├── main.rs              # Axum server setup, router composition, static file serving
├── config.rs            # Environment config (DATABASE_URL, JWT_SECRET, OPENAI_API_KEY)
├── error.rs             # Unified error type → HTTP responses
├── db.rs                # sqlx PgPool setup, health check
│
├── auth/
│   ├── mod.rs           # JWT creation/validation, password hashing (argon2)
│   ├── middleware.rs     # Tower middleware: extract & validate JWT from cookie
│   └── models.rs        # Claims, AuthUser (extracted in handlers)
│
├── routes/
│   ├── mod.rs           # Router composition: public vs protected
│   ├── auth.rs          # POST /api/auth/login, /register, /logout
│   ├── quiz.rs          # GET /random, POST /submit, /complete
│   ├── review.rs        # GET /review (wrong answers)
│   ├── mastery.rs       # GET /mastered, POST /mastery/reset
│   ├── stats.rs         # GET /stats, /categories
│   ├── coryat.rs        # POST /create, GET /:id, POST /:id/answer, /:id/complete, /history
│   ├── study.rs         # POST /generate, GET /history, /latest
│   ├── preferences.rs   # GET/POST /preferences
│   ├── questions.rs     # GET /questions/:id, POST /:id/archive, /:id/unarchive
│   └── admin.rs         # GET /users, POST /approve (admin role guard)
│
└── models/
    ├── mod.rs
    ├── user.rs          # User, NewUser, UserRole enum
    ├── question.rs      # JeopardyQuestion, QuestionAttempt
    ├── session.rs       # QuizSession
    ├── mastery.rs       # QuestionMastery
    ├── coryat.rs        # CoryatGame (game_board as serde_json::Value)
    └── study.rs         # StudyRecommendation (recommendations as serde_json::Value)
```

### Key crates

| Crate | Purpose |
|-------|---------|
| `axum` | Web framework |
| `tokio` | Async runtime |
| `sqlx` | PostgreSQL with compile-time query checking |
| `serde` / `serde_json` | Serialization |
| `jsonwebtoken` | JWT encode/decode |
| `argon2` | Password hashing |
| `bcrypt` | Verify legacy bcrypt hashes during migration |
| `tower-http` | CORS, compression, static file serving, security headers |
| `reqwest` | HTTP client for OpenAI API |
| `tracing` / `tracing-subscriber` | Structured logging |

### Auth flow

1. **Login:** verify email + argon2 hash (or bcrypt for legacy) → issue JWT in `HttpOnly`, `Secure`, `SameSite=Strict` cookie
2. **Every /api/* request** (except login/register/logout): `AuthMiddleware` extracts JWT from cookie, validates, injects `AuthUser` into handler
3. **Admin routes:** additional `RequireRole(Admin)` guard
4. **Registration:** creates user with `approved: false`, admin must approve before login works
5. **No localStorage tokens** — cookies only, eliminates the XSS vector

### Database

- Reuse existing PostgreSQL instance and schema — zero data migration
- `sqlx::query_as!` macros for all queries — checked at compile time against the real database
- Connection pool via `sqlx::PgPool` (default 10, max 20)

---

## Frontend Structure

Svelte 5 SPA built with SvelteKit (adapter-static). Outputs plain static files — no server-side rendering, no Node.js runtime in production.

### Directory layout

```
frontend/
├── src/
│   ├── app.html
│   ├── routes/
│   │   ├── +layout.svelte          # Root layout: nav, auth context
│   │   ├── +page.svelte            # Landing page (/)
│   │   ├── login/+page.svelte
│   │   ├── register/+page.svelte
│   │   ├── dashboard/+page.svelte
│   │   ├── quiz/+page.svelte
│   │   ├── review/+page.svelte
│   │   ├── mastered/+page.svelte
│   │   ├── coryat/
│   │   │   ├── +page.svelte          # Lobby
│   │   │   ├── [gameId]/+page.svelte  # Game board
│   │   │   └── history/+page.svelte
│   │   ├── study/+page.svelte
│   │   ├── settings/+page.svelte
│   │   └── admin/+page.svelte
│   │
│   ├── lib/
│   │   ├── api.ts            # Fetch wrapper: base URL, credentials, error handling
│   │   ├── auth.ts           # Auth store, login/logout, session check
│   │   ├── stores/
│   │   │   ├── quiz.svelte.ts
│   │   │   ├── coryat.svelte.ts
│   │   │   └── preferences.svelte.ts
│   │   └── components/
│   │       ├── QuestionCard.svelte
│   │       ├── GameBoard.svelte
│   │       ├── StatsChart.svelte
│   │       ├── CategoryFilter.svelte
│   │       ├── MasteryBadge.svelte
│   │       ├── SessionSummary.svelte
│   │       └── Nav.svelte
│   │
│   └── app.css               # Tailwind CSS
│
├── static/                    # Favicon, PWA manifest
├── svelte.config.js           # adapter-static
├── vite.config.ts
├── tailwind.config.js
└── package.json
```

### Key decisions

- **SvelteKit with adapter-static** — file-based routing and layouts, outputs a plain static SPA
- **Tailwind CSS** — same utility classes, Jeopardy color theme carries over
- **Svelte 5 runes** — `$state`, `$derived`, `$effect` for reactivity
- **API layer** — thin `fetch` wrapper in `lib/api.ts`, cookies sent automatically (same origin)
- **No CORS** — SPA served from same origin as API
- **Keyboard shortcuts** — Space/Arrow key bindings in quiz mode via `on:keydown`
- **Charts** — Chart.js with `svelte-chartjs` (replaces Recharts)
- **Mobile-first responsive** — same Tailwind breakpoint approach

### Auth in the SPA

- On app load, `GET /api/auth/me` checks if the cookie session is valid
- If valid, populate auth store with user info
- If not, redirect to `/login`
- Protected routes check auth store in layout `load` functions

---

## API Endpoints

### Public (no auth)

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/api/auth/register` | Create account (pending approval) |
| `POST` | `/api/auth/login` | Authenticate, set JWT cookie |
| `POST` | `/api/auth/logout` | Clear JWT cookie |
| `GET` | `/api/auth/me` | Check current session |

### Authenticated

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/quiz/random` | Random question (with category/game-type filters) |
| `POST` | `/api/quiz/submit` | Submit answer |
| `POST` | `/api/quiz/complete` | End session, return summary |
| `GET` | `/api/questions/:id` | Single question by ID |
| `POST` | `/api/questions/:id/archive` | Archive a question |
| `POST` | `/api/questions/:id/unarchive` | Unarchive a question |
| `GET` | `/api/review` | Wrong answers for current user |
| `GET` | `/api/mastered` | Mastered questions |
| `POST` | `/api/mastery/reset` | Reset mastery on a question |
| `GET` | `/api/stats` | User statistics |
| `GET` | `/api/categories` | Available categories with counts |
| `GET` | `/api/preferences` | User game-type filter preferences |
| `PUT` | `/api/preferences` | Update preferences |
| `POST` | `/api/coryat` | Create new Coryat game |
| `GET` | `/api/coryat/:id` | Get game state |
| `POST` | `/api/coryat/:id/answer` | Submit answer for a board cell |
| `POST` | `/api/coryat/:id/complete` | Complete a game |
| `GET` | `/api/coryat/history` | Completed game history |
| `POST` | `/api/study/generate` | Generate AI study recommendations |
| `GET` | `/api/study/history` | Past recommendations |
| `GET` | `/api/study/latest` | Most recent recommendation |

### Admin (requires admin role)

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/admin/users` | List all users |
| `POST` | `/api/admin/approve` | Approve a pending user |

### Changes from current app

- Dropped `/api/auth/persistent-token` and `/api/auth/csrf` — JWT cookies replace both
- Archive/unarchive moved under `/api/questions/:id/` for REST consistency
- `PUT` for preferences (idempotent update)

### Security headers (global via tower-http)

- `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'`
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `Referrer-Policy: strict-origin-when-cross-origin`
- `Permissions-Policy: camera=(), microphone=(), geolocation=()`

---

## Deployment & Docker

### Multi-stage Dockerfile

```
Stage 1: frontend-build
  - Node 22 Alpine
  - Install deps, build SvelteKit static → /app/frontend/build/

Stage 2: rust-build
  - rust:bookworm
  - Copy frontend build into static assets directory
  - cargo build --release → single binary

Stage 3: runtime
  - gcr.io/distroless/cc-debian12
  - Copy binary only
  - EXPOSE 3000
  - ENTRYPOINT ["/app/server"]
```

Final image contains: the Rust binary (~15-25MB), Svelte static assets, glibc. Nothing else.

### Docker Compose

```yaml
services:
  jeopardy:
    image: ghcr.io/ebertx/jeopardy-training-app:latest
    restart: unless-stopped
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    environment:
      - DATABASE_URL
      - JWT_SECRET
      - OPENAI_API_KEY
    networks:
      - internal
    healthcheck:
      test: ["/app/server", "--healthcheck"]
      interval: 30s

  cloudflared:
    image: cloudflare/cloudflared:latest
    restart: unless-stopped
    command: tunnel run
    environment:
      - TUNNEL_TOKEN
    networks:
      - internal

networks:
  internal:
    driver: bridge
```

### Container hardening

- `read_only: true` — immutable filesystem
- `cap_drop: ALL` — no Linux capabilities
- `no-new-privileges` — no privilege escalation
- Isolated `internal` network — only cloudflared and the app communicate
- No published ports — zero `0.0.0.0` bindings
- Secrets in `.env` on server, never in image or git

### Cloudflare Tunnel

- Runs as a container in the same Docker Compose stack (not installed on the host)
- Routes `jeopardy.ebertx.com` → `http://jeopardy:3000` (Docker internal network)
- Handles TLS termination, WAF, DDoS mitigation
- No inbound ports required on Tower
- Tunnel token stored in `.env` on server alongside other secrets

---

## Migration Strategy

### Database

Zero migration. Same PostgreSQL instance, same tables, same schema. `sqlx` queries target existing table and column names exactly.

One post-cutover cleanup: drop the `auth_sessions` table (NextAuth artifact, no longer needed).

### Password hash migration

Existing passwords use bcrypt (`bcryptjs`). New app uses argon2.

- On login, detect hash format by prefix (`$2a$`/`$2b$` = bcrypt, `$argon2` = argon2)
- If bcrypt: verify with `bcrypt` crate, re-hash with `argon2`, update the row
- New registrations always use `argon2`
- Users migrate transparently by logging in

### Secrets rotation (required)

All current secrets are committed to git and must be rotated:

- New `JWT_SECRET` (replaces `NEXTAUTH_SECRET`)
- New PostgreSQL password
- New OpenAI API key
- Remove `.env` from git history with `git filter-repo`

### Cutover plan

1. Build and push new image to GHCR
2. Rotate all secrets on Tower
3. Set up Cloudflare Tunnel
4. Bind PostgreSQL to `127.0.0.1` or Docker internal network only
5. Deploy new container with docker-compose
6. Verify all features
7. Remove old Traefik routing for `jeopardy.ebertx.com`
8. Drop `auth_sessions` table
9. Update container security monitor for new container name

### Rollback

Old image remains in GHCR. Can redeploy with a patched `next` version while debugging the Rust build.

---

## Feature Parity Checklist

All existing features are preserved:

- [ ] Landing page
- [ ] Registration with admin approval
- [ ] Login/logout with JWT cookies
- [ ] Dashboard with stats, charts, category breakdown
- [ ] Quiz mode with category/game-type filtering
- [ ] Question prefetching
- [ ] Keyboard shortcuts (Space, arrows)
- [ ] Session summary modal
- [ ] Question archival
- [ ] Review wrong answers with mastery progress
- [ ] Mastered questions view with reset
- [ ] Coryat scoring (lobby, game board, history)
- [ ] AI study recommendations (OpenAI GPT-4o)
- [ ] Study recommendation history
- [ ] User preferences (game type filters)
- [ ] Admin panel (user list, approval)
- [ ] Settings page
- [ ] Mobile-responsive design
- [ ] PWA support (home screen installable)
