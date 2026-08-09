FROM postgres:17

# Build-time defaults: avoid CPU-specific instruction sets and make deterministic
ENV DEBIAN_FRONTEND=noninteractive
# Use conservative compile flags so the produced shared object is portable
ENV CFLAGS="-O2 -fPIC -mtune=generic"
# Run make single-threaded to avoid parallel build surprises in constrained CI/dev hosts
ENV MAKEFLAGS="-j1"

# Install build dependencies and compile/install pgvector extension
RUN apt-get update \
  && apt-get install -y --no-install-recommends build-essential ca-certificates git postgresql-server-dev-17 \
  && git clone https://github.com/pgvector/pgvector.git /tmp/pgvector \
  && cd /tmp/pgvector && make CFLAGS="$CFLAGS" MAKEFLAGS="$MAKEFLAGS" && make install CFLAGS="$CFLAGS" \
  && rm -rf /tmp/pgvector \
  && apt-get purge -y --auto-remove build-essential git \
  && rm -rf /var/lib/apt/lists/*

# Use default postgres entrypoint/cmd
