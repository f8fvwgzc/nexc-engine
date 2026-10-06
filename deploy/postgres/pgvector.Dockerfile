# PostgreSQL 17 with the pgvector extension, for vector search over memories and documents.
# The same as pgvector/pgvector:pg17, built from the official image and the PGDG package:
#   docker build -t nexc-postgres-pgvector:17 -f deploy/postgres/pgvector.Dockerfile deploy/postgres
FROM postgres:17
RUN apt-get update \
    && apt-get install -y --no-install-recommends postgresql-17-pgvector \
    && rm -rf /var/lib/apt/lists/*
