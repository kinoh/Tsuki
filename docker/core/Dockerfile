# Production stage
FROM node:22-alpine AS runtime

WORKDIR /app

# Install build dependencies for native modules
RUN apk add --no-cache python3 make g++ sqlite

# Create non-root user for security first
ARG UID=10001
RUN addgroup -g 1001 -S nodejs && \
    adduser -S tsuki -u "${UID}" -G nodejs

# Install production dependencies with correct ownership
COPY --chown=tsuki:nodejs core/package*.json core/pnpm-lock.yaml ./
COPY --chown=tsuki:nodejs core/.pnpmrc ./
RUN corepack enable pnpm && pnpm install --prod --frozen-lockfile && pnpm rebuild better-sqlite3

# Copy source code with correct ownership
COPY --chown=tsuki:nodejs core/src ./src
COPY --chown=tsuki:nodejs core/tsconfig.json ./
COPY --chown=tsuki:nodejs core/scripts/generate_opml.js ./scripts/

# Create data directory with correct ownership
RUN mkdir -p /data && chown -R tsuki:nodejs /data

# Switch to non-root user
USER tsuki

# Generate OPML file during build
RUN node scripts/generate_opml.js /data/rss_feeds.opml

# Set environment variables
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
ENV NODE_ENV=production
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
