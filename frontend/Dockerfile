# syntax=docker/dockerfile:1.7
# nexc frontend: Vite build served by unprivileged nginx (port 8080), proxying /api to the backend.
# Build: docker build -t nexc-engine-frontend ./frontend

ARG NODE_IMAGE=node:22-bookworm-slim
ARG NGINX_IMAGE=nginxinc/nginx-unprivileged:1.28-alpine

# ---------------------------------------------------------------------------------------------
FROM ${NODE_IMAGE} AS build
ARG PNPM_VERSION=10.18.1
ENV PNPM_HOME=/pnpm \
    PATH=/pnpm:$PATH \
    CI=true
RUN npm install --global --no-fund --no-audit "pnpm@${PNPM_VERSION}" \
 && pnpm config set store-dir /pnpm/store
WORKDIR /app

# .pnpmfile.cjs rewrites dependencies (its checksum is in the lockfile), so it must be present
# for --frozen-lockfile; the globs keep the build working if either optional file disappears.
COPY package.json pnpm-lock.yaml .pnpmfile.cj[s] .npmr[c] ./
RUN --mount=type=cache,id=pnpm-store,target=/pnpm/store \
    pnpm install --frozen-lockfile

COPY . .
ARG VITE_API_BASE_URL=/api/v1
ENV VITE_API_BASE_URL=${VITE_API_BASE_URL}
RUN pnpm build

# ---------------------------------------------------------------------------------------------
FROM ${NGINX_IMAGE} AS runtime
LABEL org.opencontainers.image.title="nexc-engine-frontend" \
      org.opencontainers.image.description="nexc-engine web UI (nginx)" \
      org.opencontainers.image.source="https://github.com/f8fvwgzc/nexc-engine" \
      org.opencontainers.image.licenses="MIT"

COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/dist /usr/share/nginx/html

# nginx-unprivileged runs as uid 101 and writes only to /tmp (pid, temp paths).
USER 101
EXPOSE 8080

HEALTHCHECK --interval=15s --timeout=5s --start-period=5s --retries=3 \
  CMD ["wget", "-q", "-T", "3", "-O", "/dev/null", "http://127.0.0.1:8080/healthz"]
