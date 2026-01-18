# Ruby Application
# Build: ./run_container.sh build -f example-forge-files/Forgefile.ruby -t ruby-app:v1.0
# Run:   ./run_container.sh run ruby-app:v1.0

FROM alpine:3.19

# Install Ruby and Bundler
RUN apk add --no-cache ruby ruby-bundler ruby-dev build-base

WORKDIR /app

# Copy Gemfile first (for better caching)
COPY Gemfile /app/
COPY Gemfile.lock /app/

# Install dependencies
RUN bundle install --without development test

# Copy application code
COPY app.rb /app/

ENTRYPOINT ["ruby", "app.rb"]
