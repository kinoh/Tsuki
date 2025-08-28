# Rust build stage
FROM rust:1.89-alpine AS rust-builder

WORKDIR /build

# Install musl target and tools for static linking
RUN apk add --no-cache musl-dev \
    && rustup target add x86_64-unknown-linux-musl

# Copy Rust source code
COPY mcp/structured-memory ./mcp/structured-memory

# Build structured-memory with caching
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/mcp/structured-memory/target \
    cd mcp/structured-memory && \
    cargo build --release --target x86_64-unknown-linux-musl && \
    cp target/x86_64-unknown-linux-musl/release/structured-memory /build/structured-memory

# Production stage
FROM node:22-alpine AS runtime

WORKDIR /app

# Install build dependencies for native modules and tools
RUN apk add --no-cache \
    python3 \
    make \
    g++ \
    sqlite \
    sqlite-dev \
    npm \
    && corepack enable pnpm

# Create non-root user for security first
ARG UID=10001
RUN addgroup -g 1001 -S nodejs && \
    adduser -S tsuki -u "${UID}" -G nodejs

# Create data directory and set app directory ownership
RUN mkdir -p /data && chown -R tsuki:nodejs /data /app

# Set environment for proper native module compilation
ENV NODE_ENV=production
ENV npm_config_build_from_source=true

# Switch to non-root user before installing dependencies
USER tsuki

# Install production dependencies with correct ownership
COPY --chown=tsuki:nodejs core/package*.json core/pnpm-lock.yaml ./
COPY --chown=tsuki:nodejs core/.pnpmrc ./
COPY --chown=tsuki:nodejs core/patches/ ./patches/

# Install dependencies as non-root user (pnpm handles native modules correctly)
RUN --mount=type=cache,id=pnpm,target=/pnpm/store \
    pnpm install --prod --frozen-lockfile

# Allow build scripts for native modules (pnpm will handle better-sqlite3 automatically)
RUN pnpm config set script-shell /bin/sh

# Copy structured-memory binary from build stage
COPY --from=rust-builder --chown=tsuki:nodejs /build/structured-memory ./bin/structured-memory

# Copy source code with correct ownership
COPY --chown=tsuki:nodejs core/src ./src
COPY --chown=tsuki:nodejs core/tsconfig.json ./
COPY --chown=tsuki:nodejs core/scripts/generate_opml.js ./scripts/

# Generate OPML file during build
RUN node scripts/generate_opml.js /data/rss_feeds.opml

# Set environment variables
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
ENV PORT=2953

# Build arguments for versioning
ARG GIT_HASH
ENV GIT_HASH=$GIT_HASH

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD node -e "require('http').request({port: process.env.PORT || 2953, path: '/'}, (res) => process.exit(res.statusCode === 200 ? 0 : 1)).on('error', () => process.exit(1)).end()"

# Expose port
EXPOSE 2953

# Start the application with tsx
CMD ["pnpm", "run", "start:prod"]
