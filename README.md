[![Build Status](https://travis-ci.org/flodiebold/cautious-tribble.svg?branch=master)](https://travis-ci.org/flodiebold/cautious-tribble)

# Cautious tribble (real name TBD)
It deploys your stuff to Kubernetes.

## Features
 - needs only a git repo and Kubernetes
 - small footprint
 - robust; all state in the git repo (can update itself)
 - deploys all kinds of resources
 - transitions stuff to the next environment when it works (or on a time schedule)

[TODO maybe a screenshot or two once ui is there]

## How it works
[TODO]

### Structure of the resource repo
 - top-level, there is one folder per environment, e.g. `dev`, `pp`, `prod`.
 - below that, there can be the following folders:
   - `deployable`: Full Kubernetes resource files (currently only in yaml format) in an arbitrary folder structure.
   - `base` and `version`: These belong together; for each file `version/x`, there should be a corresponding `base/x`. The deployer merges the two files together, getting a complete resource to deploy to Kubernetes. (Currently, this merge happens by just replacing the string `$version` in all fields in the base file by the content of the map value `version` in the version file, but that's a placeholder algorithm.) This way, it is possible to have part of a resource controlled by transitions, going through the environments, and the rest of the resource varying by environment. The transitioner only mirrors the contents of the `version` folder from one environment to the next.
   - `transitions.yaml` contains state related to transitions; concretely when a recurring transition is scheduled next.
   - `locks.yaml` contains the locking state for the environment (and per-service locking states in the future).

### Deployer configuration
[TODO]

### Transitioner configuration
[TODO]

## Contributing

### Crates
This project consists of the following crates:
 - `common`: common data structures and utilities, resource repo abstraction
 - `deployer`: the deployer makes sure the cluster matches the state of the resource repo
 - `transitioner`: the transitioner mirrors versions between environments when preconditions are met
 - `aggregator`: the aggregator watches the state of the deployer, transitioner and the resource repo itself, and provides data to the ui
 - `integration_test`: integration tests testing the combination of the services. The file `src/lib.rs` contains support code; the actual tests are in `tests/`.
 - `git_fixture`: helper crate to create git repos based on definitions in a yaml file

There is also the (WIP) typescript + React UI in `ui`.

### Useful commands
 - `cargo test --all -- --skip minikube`: Runs all tests except the ones requiring minikube
 - `test-integration-existing-minikube.sh`: Runs tests, including the minikube tests, but does not start minikube; so you need to have started it with `minikube start` before.
 - `test-integration.sh`: Starts minikube, runs all tests, and stops minikube again.
 - `cargo run -p integration_test`: Starts a 'playground' based on the integration test, with all components running. As long as the process is running, there's a directory `playground/` in which the configuration, versions repo etc. live.
 - `cd ui && yarn install && yarn start`: Runs the ui. Currently expected to be used with the playground.
 - `cargo fmt`: This project uses rustfmt, so you should run this before committing.
