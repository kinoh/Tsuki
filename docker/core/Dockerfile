# Build stage
FROM node:22-alpine AS build

WORKDIR /app

# Copy package files first for better caching
COPY core/package*.json ./

# Install all dependencies (including devDependencies for build)
RUN npm ci

# Copy source code
COPY core/ ./

# Build TypeScript to JavaScript
RUN npm run build

# Production stage
FROM node:22-alpine AS runtime

WORKDIR /app

# Create non-root user for security first
ARG UID=10001
RUN addgroup -g 1001 -S nodejs && \
    adduser -S tsuki -u "${UID}" -G nodejs

# Install only production dependencies with correct ownership
COPY --chown=tsuki:nodejs core/package*.json ./
RUN npm ci --only=production && npm cache clean --force

# Copy built application from build stage with correct ownership
COPY --chown=tsuki:nodejs --from=build /app/dist ./dist

# Switch to non-root user
USER tsuki

# Set environment variables
ENV NODE_ENV=production
ENV PORT=3000

# Build arguments for versioning
ARG GIT_HASH
ENV GIT_HASH=$GIT_HASH

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD node -e "require('http').request({port: process.env.PORT || 3000, path: '/'}, (res) => process.exit(res.statusCode === 200 ? 0 : 1)).on('error', () => process.exit(1)).end()"

# Expose port
EXPOSE 3000

# Start the application
CMD ["node", "dist/index.js"]
