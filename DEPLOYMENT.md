# Deployment — Tailscale-only on Unraid

The app is reachable **only over Tailscale** at `https://tower.tail11628.ts.net`. There is no public DNS, no exposed port on the WAN, and no Cloudflare Tunnel. TLS is provided by `tailscale serve` using MagicDNS certs.

## Architecture

```
[client on tailnet]
       │ HTTPS (MagicDNS cert)
       ▼
   tailscaled on Tower (tailscale serve)
       │ HTTP
       ▼
   127.0.0.1:3000 (jeopardy-server container)
       │
       ▼
   172.17.0.1:5432 (postgresql15 on Tower, via docker bridge gateway)
```

Container is hardened: `read_only`, `cap_drop: ALL`, `no-new-privileges`, distroless runtime image (no shell). Port 3000 is bound to the host's loopback only — nothing on the LAN can reach it.

## Deploy Flow

1. Push to `main` → GitHub Actions builds and pushes `ghcr.io/ebertx/jeopardy-training-app:latest`
2. Watchtower (already running on Tower) pulls the new image and restarts the container
3. `tailscale serve` keeps proxying transparently — no restart needed there

## One-Time Setup on Tower

### 1. Create app directory + `.env`

```bash
mkdir -p /mnt/user/appdata/jeopardy-training-app
cd /mnt/user/appdata/jeopardy-training-app
```

Write `.env` (replace placeholders — do **not** commit this file):

```
DATABASE_URL=postgres://ebertx:C&M24postgres@172.17.0.1:5432/jeopardy
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
docker compose up -d
docker logs jeopardy-server
```

Verify it's listening on loopback only:

```bash
ss -tlnp | grep :3000
# Should show: 127.0.0.1:3000   (NOT 0.0.0.0:3000)
```

### 4. Front it with `tailscale serve`

This makes `https://tower.tail11628.ts.net` proxy to `localhost:3000` over the tailnet, with a real cert from Tailscale's CA.

```bash
tailscale serve --bg --https=443 http://localhost:3000
tailscale serve status
```

To remove later:

```bash
tailscale serve reset
```

### 5. Verify from another tailnet device

```bash
curl -I https://tower.tail11628.ts.net/api/health
# 200 OK
```

Open the app at `https://tower.tail11628.ts.net` from a phone/laptop on the tailnet.

## Update Flow (after first deploy)

Just push to `main`. Watchtower handles the rest:

```bash
docker logs --tail 50 watchtower | grep jeopardy
```

## Secret Rotation

| Secret | When to rotate | How |
|---|---|---|
| `JWT_SECRET` | If suspected compromise; invalidates all sessions | `openssl rand -base64 48`, update `.env`, `docker compose up -d` |
| `OPENAI_API_KEY` | After last incident; user-side at platform.openai.com | Update `.env`, restart container |
| Postgres password | Coordinate with `polymarket-tracker` and `health-ingester` (shared user) | Do not rotate without updating consumers |

## Open Hardening Items (deferred — affects shared infra)

- **PostgreSQL binds `0.0.0.0:5432`.** Consider restricting to `172.17.0.1:5432` (docker bridge) so it's only reachable by containers on Tower. Requires updating `polymarket-tracker` and `health-ingester` if they're going through any non-bridge path.
- **Drop `auth_sessions` table.** Was used by the old NextAuth deployment; new app uses stateless JWTs.
- **Scrub historical secrets from git.** `.env` and an old `NEXTAUTH_SECRET` are in repo history. Use `git filter-repo` if/when you want a clean history.

## Troubleshooting

**`tailscale serve` returns a cert error**
First request after enabling MagicDNS HTTPS can take 30–60s while the cert is provisioned. Try again.

**Container restarts in a loop**
```bash
docker logs jeopardy-server | tail -50
```
Most likely a missing env var (`DATABASE_URL`, `JWT_SECRET`, `OPENAI_API_KEY` are all required).

**`401 Unauthorized` on every request after a deploy**
`JWT_SECRET` changed — all existing sessions are invalidated. Users need to log in again. Expected on first deploy and any rotation.

**Database connection refused**
```bash
docker exec jeopardy-server /app/server --healthcheck
```
If the binary is up but DB is unreachable, check `docker exec jeopardy-server cat /etc/resolv.conf` and try `172.17.0.1` from another container. Distroless has no shell — debug from a sidecar if needed.
