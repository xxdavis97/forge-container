# Java Application (Pre-built JAR)
# Build: ./run_container.sh build -f example-forge-files/Forgefile.java -t java-app:v1.0
# Run:   ./run_container.sh run java-app:v1.0

FROM alpine:3.19

# Install OpenJDK 17 runtime
RUN apk add --no-cache openjdk17-jre

# Set JAVA_HOME
ENV JAVA_HOME=/usr/lib/jvm/java-17-openjdk

WORKDIR /app

# Copy pre-built JAR file
COPY app.jar /app/

ENTRYPOINT ["java", "-jar", "app.jar"]
