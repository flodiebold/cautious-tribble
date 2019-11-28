#!/usr/bin/env bash

set -e

eval "$(minikube docker-env)" || echo "minikube docker-env failed (not needed with vm-driver=none)"

echo "Building services..."
(cd deployer && cargo build)
(cd transitioner && cargo build)

cd integration_test

echo "Building test images..."
images/build.sh

echo "Running tests..."
cargo test "$@"
