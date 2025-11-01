# Deployment Guide - Unraid with Traefik

This guide sets up automated deployments from GitHub to your Unraid server using Traefik, Watchtower, and GitHub Container Registry.

## Overview

**Deployment Flow:**
1. Push code to `main` branch on GitHub
2. GitHub Actions builds Docker image
3. Image pushed to GitHub Container Registry (GHCR)
4. Watchtower on Unraid detects new image
5. Container automatically updated and restarted
6. Accessible via Traefik reverse proxy at jeopardy.ebertx.com

---

## One-Time Setup (For All Apps)

### 1. Configure Traefik on Unraid (Installed via Community Apps)

Since Traefik is installed via Unraid Community Apps and uses ports 8001/44301, we need to configure it for Cloudflare DNS challenge.

**Create `/mnt/user/appdata/traefik/traefik.yml`:**

```yaml
api:
  dashboard: true
  insecure: true

entryPoints:
  web:
    address: ":80"
    http:
      redirections:
        entryPoint:
          to: websecure
          scheme: https
  websecure:
    address: ":443"

certificatesResolvers:
  letsencrypt:
    acme:
      email: ebertx@gmail.com
      storage: /letsencrypt/acme.json
      dnsChallenge:
        provider: cloudflare
        resolvers:
          - "1.1.1.1:53"
          - "1.0.0.1:53"

providers:
  docker:
    endpoint: "unix:///var/run/docker.sock"
    exposedByDefault: false
    network: traefik

log:
  level: INFO
```

**Add Cloudflare credentials to Traefik container:**

Edit your Traefik container in Unraid and add these environment variables:
- `CF_API_EMAIL=ebertx@gmail.com`
- `CF_DNS_API_TOKEN=e1HHBX5kFli0kwHm4XNN-HDboizF9_3TQpVCrxyW`

**Restart Traefik** after making these changes.

### 2. Install Watchtower on Unraid

In Unraid Community Applications, search for "Watchtower" or create manually:

```bash
docker run -d \
  --name watchtower \
  --restart unless-stopped \
  -v /var/run/docker.sock:/var/run/docker.sock \
  containrrr/watchtower \
  --interval 300 \
  --cleanup \
  --label-enable
```

This checks for updates every 5 minutes (300 seconds).

### 3. Configure DNS and Port Forwarding

**In Cloudflare DNS:**
- Type: A Record
- Name: `jeopardy`
- Value: Your public IP address
- Proxy status: DNS only (gray cloud, not proxied)
- TTL: Auto

**Configure Router Port Forwarding:**

Since Traefik runs on ports 8001/44301, you need to forward standard ports to these:
- External Port 80 → Unraid IP:8001
- External Port 443 → Unraid IP:44301

This allows public internet traffic on standard ports to reach Traefik.

---

## Per-App Deployment (This App)

### 1. Enable GitHub Container Registry

Your GitHub Actions workflow is already configured. On first push to `main`, it will:
- Build the Docker image
- Push to `ghcr.io/ebertx/jeopardy-training-app`

**Make repository package public** (optional, but recommended for easier Unraid access):
1. Go to https://github.com/ebertx/jeopardy-training-app/packages
2. Find your package
3. Package settings → Change visibility → Public

### 2. Deploy on Unraid

Create directory on Unraid:
```bash
mkdir -p /mnt/user/appdata/jeopardy-training-app
cd /mnt/user/appdata/jeopardy-training-app
```

Create `.env` file:
```bash
cat > .env << 'EOF'
DATABASE_URL=postgresql://ebertx:C&M24postgres@100.92.27.16/jeopardy
NEXTAUTH_SECRET=j5keXeQun/N0X9bxMgQgkfoSAMnbTGI0vA7F05H+qTU=
NEXTAUTH_URL=https://jeopardy.ebertx.com
EOF
```

Copy `docker-compose.yml` from this repository to the server:
```bash
# From your local machine:
scp docker-compose.yml root@unraid-ip:/mnt/user/appdata/jeopardy-training-app/
```

The domain is already configured as `jeopardy.ebertx.com` in docker-compose.yml.

Start the container:
```bash
cd /mnt/user/appdata/jeopardy-training-app
docker-compose up -d
```

### 3. Verify Deployment

Check container is running:
```bash
docker ps | grep jeopardy
docker logs jeopardy-training-app
```

Check Traefik dashboard: `http://unraid-ip:8080`

Access your app: `https://jeopardy.ebertx.com`

---

## How to Deploy Updates

**It's automatic!** Just:
```bash
git add .
git commit -m "Your changes"
git push origin main
```

GitHub Actions will:
1. Build new image (2-5 minutes)
2. Push to GHCR
3. Watchtower detects new image (within 5 minutes)
4. Pulls and restarts container automatically

**Check deployment status:**
- GitHub Actions: https://github.com/ebertx/jeopardy-training-app/actions
- Watchtower logs: `docker logs watchtower`
- App logs: `docker logs jeopardy-training-app`

---

## Troubleshooting

### GitHub Actions fails to push image
**Issue:** Permission denied when pushing to GHCR

**Fix:** Make sure GitHub Actions has package write permissions:
1. Repo Settings → Actions → General
2. Workflow permissions → Read and write permissions
3. Save

### Watchtower not updating
**Issue:** Container not updating automatically

**Check:**
```bash
docker logs watchtower
```

**Ensure:**
- Image label matches: `com.centurylinklabs.watchtower.enable=true`
- Watchtower has access to docker socket
- Image is actually different (check GitHub Actions)

### Can't access via domain
**Issue:** Domain not resolving or SSL errors

**Check:**
1. DNS points to correct Tailscale IP
2. Traefik is running: `docker ps | grep traefik`
3. Check Traefik logs: `docker logs traefik`
4. Verify Traefik dashboard shows your router
5. Ensure you're connected to Tailscale

### Database connection fails
**Issue:** App can't connect to PostgreSQL

**Check:**
1. Database IP is correct (100.92.27.16)
2. Database is accessible from container:
   ```bash
   docker exec jeopardy-training-app wget -O- 100.92.27.16:5432
   ```
3. Check .env file has correct DATABASE_URL

---

## Reusable Pattern for Other Apps

This setup can be reused for any app! Just:

1. **Copy these files to new project:**
   - `Dockerfile` (adjust if not Next.js)
   - `.dockerignore`
   - `docker-compose.yml` (change app name and domain)
   - `.github/workflows/deploy.yml` (no changes needed!)

2. **Update docker-compose.yml:**
   - Change `image: ghcr.io/yourusername/new-app:latest`
   - Change `container_name: new-app`
   - Change `Host(` )`new-app.yourdomain.com`)`
   - Update Traefik router/service names

3. **Deploy:**
   - Push to GitHub
   - Copy .env and docker-compose.yml to Unraid
   - Run `docker-compose up -d`
   - Done!

---

## Security Notes

- ✅ App only accessible via Tailscale (Traefik on Tailscale IP)
- ✅ SSL certificates automatically managed by Let's Encrypt
- ✅ Database credentials never in GitHub (use .env)
- ✅ GHCR images can be private
- ✅ Watchtower only updates containers with explicit label

**For public apps:** Remove Tailscale requirement and point DNS to public IP instead.
