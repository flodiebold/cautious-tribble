#!/usr/bin/env bash

set -e

echo "Starting minikube..."
minikube start

stopMinikube() {
    minikube stop
}
trap stopMinikube EXIT

./test-integration-existing-minikube.sh
