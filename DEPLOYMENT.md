# Deployment — Tailscale-only on Unraid

The app is reachable **only over Tailscale** at `http://tower.tail11628.ts.net:3000`. The container's port 3000 is bound to Tower's Tailscale IP (`100.92.27.16`) — unreachable from the LAN or WAN. WireGuard (Tailscale's transport) encrypts the traffic, so plain HTTP is fine inside the tailnet.

## Architecture

```
[client on tailnet]
       │ HTTP over WireGuard (encrypted by Tailscale)
       ▼
   100.92.27.16:3000 (Tower's Tailscale IP, host port)
       │
       ▼
   jeopardy-server container (port 3000)
       │
       ▼
   host.docker.internal:5432 (postgresql15 on Tower, via host-gateway in extra_hosts)
```

Container is hardened: `read_only`, `cap_drop: ALL`, `no-new-privileges`, distroless runtime image (no shell). The port binding to the Tailscale IP means nothing outside the tailnet can reach it.

## Deploy Flow

1. Push to `main` → GitHub Actions builds and pushes `ghcr.io/ebertx/jeopardy-training-app:latest`
2. Watchtower (already running on Tower) pulls the new image and restarts the container

## One-Time Setup on Tower

### 1. Create app directory + `.env`

```bash
mkdir -p /mnt/user/appdata/jeopardy-training-app
cd /mnt/user/appdata/jeopardy-training-app
```

Write `.env` (replace placeholders — do **not** commit this file):

```
DATABASE_URL=postgres://ebertx:***REDACTED***@host.docker.internal:5432/jeopardy
JWT_SECRET=<run: openssl rand -base64 48>
OPENAI_API_KEY=sk-...
```

### 2. Copy `docker-compose.yml`

From your laptop:

```bash
scp docker-compose.yml root@tower:/mnt/user/appdata/jeopardy-training-app/
```

### 3. Start the container

```bash
cd /mnt/user/appdata/jeopardy-training-app
docker-compose up -d
docker logs jeopardy-server
```

Verify it's bound to the Tailscale IP only:

```bash
ss -tlnp | grep :3000
# Should show: 100.92.27.16:3000   (NOT 0.0.0.0:3000)
```

### 4. Verify from another tailnet device

```bash
curl -I http://tower.tail11628.ts.net:3000/api/health
# 200 OK
```

Open the app at `http://tower.tail11628.ts.net:3000` from a phone/laptop on the tailnet.

## Update Flow (after first deploy)

Just push to `main`. Watchtower handles the rest:

```bash
docker logs --tail 50 watchtower | grep jeopardy
```

## Secret Rotation

| Secret | When to rotate | How |
|---|---|---|
| `JWT_SECRET` | If suspected compromise; invalidates all sessions | `openssl rand -base64 48`, update `.env`, `docker-compose up -d` |
| `OPENAI_API_KEY` | After last incident; user-side at platform.openai.com | Update `.env`, restart container |
| Postgres password | Coordinate with `polymarket-tracker` and `health-ingester` (shared user) | Do not rotate without updating consumers |

## Open Hardening Items (deferred — affects shared infra)

- **PostgreSQL binds `0.0.0.0:5432`.** Consider restricting to the docker bridge so it's only reachable by containers on Tower. Requires updating `polymarket-tracker` and `health-ingester` if they're going through any non-bridge path.
- **Drop `auth_sessions` table.** Was used by the old NextAuth deployment; new app uses stateless JWTs.
- **Scrub historical secrets from git.** `.env` and an old `NEXTAUTH_SECRET` are in repo history. Use `git filter-repo` if/when you want a clean history.

## Troubleshooting

**Container restarts in a loop**
```bash
docker logs jeopardy-server | tail -50
```
Most likely a missing env var (`DATABASE_URL`, `JWT_SECRET`, `OPENAI_API_KEY` are all required).

**`401 Unauthorized` on every request after a deploy**
`JWT_SECRET` changed — all existing sessions are invalidated. Users need to log in again. Expected on first deploy and any rotation.

**Database connection refused**
Verify `host.docker.internal` resolves from the container (it should via `extra_hosts: ["host.docker.internal:host-gateway"]`). Distroless has no shell — debug from a sidecar if needed.

**Cookie not persisting in browser**
The cookie is `HttpOnly; SameSite=Strict; Path=/` (no `Secure` flag — we're on plain HTTP over the tailnet). If sessions don't persist, check the browser's cookie storage; the cookie shouldn't be blocked since it's not cross-site.
