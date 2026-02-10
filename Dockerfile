# ===================================
# HARDENED DOCKERFILE FOR JEOPARDY APP
# ===================================

# Build stage
FROM node:20-alpine AS builder

WORKDIR /app

# Install build dependencies with security updates
RUN apk add --no-cache --update openssl

# Copy package files
COPY package*.json ./
COPY prisma ./prisma/

# Install all dependencies (including devDependencies needed for build)
RUN npm ci --ignore-scripts

# Copy source code
COPY . .

# Generate Prisma Client
RUN npx prisma generate

# Build Next.js app
RUN npm run build

# ===================================
# Production stage - HARDENED
# ===================================
FROM node:20-alpine AS runner

# Install security updates and minimal runtime dependencies
RUN apk add --no-cache --update \
    openssl \
    dumb-init \
    && rm -rf /var/cache/apk/*

WORKDIR /app

# Create non-root user with no shell and specific UID
RUN addgroup -g 1001 -S nodejs && \
    adduser -S -u 1001 -G nodejs -s /sbin/nologin nextjs

ENV NODE_ENV=production \
    NODE_OPTIONS="--max-old-space-size=512" \
    NPM_CONFIG_LOGLEVEL=error

# Copy built application with correct ownership
COPY --from=builder --chown=nextjs:nodejs /app/next.config.ts ./
COPY --from=builder --chown=nextjs:nodejs /app/public ./public
COPY --from=builder --chown=nextjs:nodejs /app/.next ./.next
COPY --from=builder --chown=nextjs:nodejs /app/node_modules ./node_modules
COPY --from=builder --chown=nextjs:nodejs /app/package.json ./package.json
COPY --from=builder --chown=nextjs:nodejs /app/prisma ./prisma

# Set restrictive permissions
RUN chmod -R 550 /app && \
    chmod -R 770 /app/.next/cache && \
    chown -R nextjs:nodejs /app

# Create read-only temp directory for Next.js
RUN mkdir -p /tmp/.next && \
    chown nextjs:nodejs /tmp/.next && \
    chmod 770 /tmp/.next

# Switch to non-root user
USER nextjs

# Expose port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
    CMD node -e "require('http').get('http://localhost:3000/api/auth/csrf', (r) => {process.exit(r.statusCode === 200 ? 0 : 1)})"

ENV PORT=3000 \
    HOSTNAME="0.0.0.0"

# Use dumb-init to handle signals properly
ENTRYPOINT ["/usr/bin/dumb-init", "--"]

# Run with restricted permissions
CMD ["node_modules/.bin/next", "start"]
