# C Application
# Build: ./run_container.sh build -f example-forge-files/Forgefile.c -t c-app:v1.0
# Run:   ./run_container.sh run c-app:v1.0

FROM alpine:3.19

# Install GCC and build tools
RUN apk add --no-cache gcc musl-dev make

WORKDIR /app

# Copy source code
COPY main.c /app/
COPY Makefile /app/

# Compile (static linking for portability)
RUN make

ENTRYPOINT ["./app"]
