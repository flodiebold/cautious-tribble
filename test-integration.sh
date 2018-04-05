#!/usr/bin/env bash

set -e

echo "Starting minikube..."
minikube start

stopMinikube() {
    minikube stop
}
trap stopMinikube EXIT

eval "$(minikube docker-env)"

echo "Building services..."
(cd deployer && cargo build)
(cd transitioner && cargo build)

cd integration_test

echo "Building test images..."
(
    cd images/example-service

    docker build . --build-arg ANSWER=23 -t exampleservice:23
)

echo "Running tests..."
cargo test
