# syntax=docker/dockerfile:1.7
# nexc frontend: Vite build served by unprivileged nginx, proxying /api to the backend: plain HTTP
# on 8080, and HTTPS with HTTP/2 and HTTP/3 (QUIC, UDP) on 8443 once a certificate is mounted at
# /etc/nginx/tls (tls.crt, tls.key; docs/adr/0001-http3-edge-caching-and-indexes.md).
# Build: docker build -t nexc-engine-frontend ./frontend

ARG NODE_IMAGE=node:22-bookworm-slim
# nginx.org's packages, which this image is built from, include ngx_http_v3_module (QUIC and
# HTTP/3) since 1.25.0; a replacement image must too, or the HTTPS listener does not start.
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
# The build output is compressed once, at the best level, so nginx sends the .gz (gzip_static)
# instead of compressing every response; -n leaves the name and time out, so the build stays
# reproducible.
RUN pnpm build \
 && find dist -type f \( -name '*.js' -o -name '*.mjs' -o -name '*.css' -o -name '*.html' \
      -o -name '*.svg' -o -name '*.json' -o -name '*.txt' -o -name '*.webmanifest' -o -name '*.ttf' \) \
      -size +1k -exec gzip -9 -k -n {} +

# ---------------------------------------------------------------------------------------------
FROM ${NGINX_IMAGE} AS runtime
LABEL org.opencontainers.image.title="nexc-engine-frontend" \
      org.opencontainers.image.description="nexc-engine web UI (nginx)" \
      org.opencontainers.image.source="https://github.com/f8fvwgzc/nexc-engine" \
      org.opencontainers.image.licenses="MIT"

COPY nginx.conf /etc/nginx/conf.d/default.conf
# The site both listeners serve, the edge cache of the public documents, the HTTPS listener.
COPY nginx/ /etc/nginx/nexc/
# Before nginx starts: the CSP's frame-src (onto-intelli's origin, NEXC_ONTO_PUBLIC_URL), and the
# HTTPS listener when a certificate is mounted.
COPY --chmod=0755 docker-entrypoint.d/40-nexc-frame-src.sh docker-entrypoint.d/41-nexc-tls.sh \
     /docker-entrypoint.d/
COPY --from=build /app/dist /usr/share/nginx/html

# nginx-unprivileged runs as uid 101 and writes only to /tmp (pid, temp paths).
USER 101
EXPOSE 8080 8443 8443/udp

HEALTHCHECK --interval=15s --timeout=5s --start-period=5s --retries=3 \
  CMD ["wget", "-q", "-T", "3", "-O", "/dev/null", "http://127.0.0.1:8080/healthz"]
