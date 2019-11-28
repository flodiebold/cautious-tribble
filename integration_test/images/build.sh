#!/usr/bin/env bash

set -e

(
    cd $(dirname $0)/example-service
    docker build . --build-arg ANSWER=23 -t exampleservice:23
    docker build . --build-arg ANSWER=42 -t exampleservice:42
)
