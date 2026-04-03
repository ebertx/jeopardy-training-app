# Stage 1: Build frontend
FROM node:22-alpine AS frontend-build
WORKDIR /app/frontend
COPY frontend/package*.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Stage 2: Build Rust backend
FROM rust:bookworm AS rust-build
WORKDIR /app

# Copy Cargo files for dependency caching
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src target/release/jeopardy-server target/release/deps/jeopardy*

# Copy real source + frontend build
COPY backend/src ./src
COPY --from=frontend-build /app/frontend/build ./static

# Build with SQLX_OFFLINE=true (use cached query metadata)
COPY backend/.sqlx ./.sqlx
ENV SQLX_OFFLINE=true
RUN cargo build --release

# Stage 3: Runtime (distroless)
FROM gcr.io/distroless/cc-debian12
COPY --from=rust-build /app/target/release/jeopardy-server /app/server
COPY --from=rust-build /app/static /app/static
ENV STATIC_DIR=/app/static
EXPOSE 3000
ENTRYPOINT ["/app/server"]
