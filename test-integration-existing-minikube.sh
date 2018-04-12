#!/usr/bin/env bash

set -e

eval "$(minikube docker-env)"

echo "Building services..."
(cd deployer && cargo build)
(cd transitioner && cargo build)

cd integration_test

echo "Building test images..."
(
    cd images/example-service

    docker build . --build-arg ANSWER=23 -t exampleservice:23
    docker build . --build-arg ANSWER=42 -t exampleservice:42
)

echo "Running tests..."
cargo test
