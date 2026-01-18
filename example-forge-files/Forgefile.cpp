# C++ Application
# Build: ./run_container.sh build -f example-forge-files/Forgefile.cpp -t cpp-app:v1.0
# Run:   ./run_container.sh run cpp-app:v1.0

FROM alpine:3.19

# Install G++ and build tools
RUN apk add --no-cache g++ musl-dev make

WORKDIR /app

# Copy source code
COPY main.cpp /app/
COPY Makefile /app/

# Compile (static linking for portability)
RUN make

ENTRYPOINT ["./app"]
