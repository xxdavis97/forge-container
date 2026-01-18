# Go Application (Build from source)
# Build: ./run_container.sh build -f example-forge-files/Forgefile.go -t go-app:v1.0
# Run:   ./run_container.sh run go-app:v1.0
#
# Note: Go compiles to a static binary, so multi-stage builds
# would allow a much smaller final image (just the binary).

FROM alpine:3.19

# Install Go
RUN apk add --no-cache go

WORKDIR /app

# Copy go.mod first (for better caching)
COPY go.mod /app/
COPY go.sum /app/

# Download dependencies
RUN go mod download

# Copy source code
COPY main.go /app/

# Build static binary
RUN CGO_ENABLED=0 go build -o app main.go

ENTRYPOINT ["./app"]
