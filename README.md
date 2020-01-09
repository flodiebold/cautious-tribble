# Cautious tribble (real name TBD)

[![Build Status](https://travis-ci.org/flodiebold/cautious-tribble.svg?branch=master)](https://travis-ci.org/flodiebold/cautious-tribble)

It deploys your stuff to Kubernetes.

## Features
 - needs only a git repo and Kubernetes
 - small footprint
 - robust; all state in the git repo (can update itself)
 - deploys all kinds of resources
 - transitions stuff to the next environment when it works (or on a time schedule)

[TODO maybe a screenshot or two once ui is there]

## How it works
The central piece of a DM system is the *resource repository*, a git repository containing all state and the configuration of the deployable resources.
 
This project consists of the following parts:
 - the *deployer* makes sure the cluster matches the state of the resource repo
 - the *transitioner* mirrors versions between environments when preconditions are met
 - the *aggregator* watches the state of the deployer, transitioner and the resource repo itself, and provides data to the ui
 - the *ui* allows inspecting the state of the deployments, locking services and environments, etc.

### Structure of the resource repo
 - top-level, there is one folder per environment, e.g. `dev`, `pp`, `prod`.
 - below that, there can be the following folders:
   - `deployable`: Full Kubernetes resource files (currently only in yaml format) in an arbitrary folder structure.
   - `base` and `version`: These belong together; for each file `version/x`, there should be a corresponding `base/x`. The deployer merges the two files together, getting a complete resource to deploy to Kubernetes. (Currently, this merge happens by just replacing the string `$version` in all fields in the base file by the content of the map value `version` in the version file, but that's a placeholder algorithm.) This way, it is possible to have part of a resource controlled by transitions, going through the environments, and the rest of the resource varying by environment. The transitioner only mirrors the contents of the `version` folder from one environment to the next.
   - `deployers.yaml` contains the deployer configuration.
   - `transitions.yaml` contains the transition configuration.
   - `transition_state.yaml` contains state related to transitions; concretely when a recurring transition is scheduled next.
   - `locks.yaml` contains the locking state for the environment (and per-service locking states in the future).

### Example flow of a new service version
[TODO]
 - your CI (e.g. Jenkins) builds a docker image and pushes it to a registry. Then it calls the aggregator to inform it about the newly available version (including a changelog).
 - the aggregator makes a new commit in the resource repository, adding the new resource version (in the `latest` environment) and including the changelog in the commit message.
 - the transitioner notices there is a new version available in the `latest` environment. Since it is configured to mirror everything from `latest` to `dev`, it makes a new transition commit setting the deployed version of your service to the newly created one.
 - the deployer notices that the version of your service specified in the resource repository for `dev` is not actually deployed, so it calls Kubernetes to apply the new configuration.
 - the transitioner is configured to mirror everything from `dev` to `pp`, but only if it is deployed successfully; so it regularly checks in with the deployer to see whether everything on `dev` is deployed cleanly. Once that is the case, it makes a transition from `dev` to `pp` including all services that changed in the meantime.
 - the deployer notices again that there is a new version, this time for `pp`, and deploys that.
 - and so on...
 
### Configuration
Each service (deployer, transitioner, aggregator) is configured with a yaml file. The following fields are common to all three:
 - `versions_url`: the git URL for the resource repository
 - `versions_checkout_path`: the path where the resource repository should be checked out
 - `api_port`: the port to use for the REST API
 
The deployer takes the following additional options:
 - `deployers`: this configures what to deploy where. [TODO]
 
The transitioner takes the following additional options:
 - `transitions`: a list of transitions between environments. [TODO]
 - `deployer_url`: the URL under which the deployer can be reached.
 
The aggregator takes the following fields:
 - `deployer_url`: the URL under which the deployer can be reached.
 - `aggregator_url`: the URL under which the aggregator can be reached.

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
 - `cargo test --all`: Runs all tests except the ones requiring minikube
 - `test-integration-existing-minikube.sh`: Runs tests, including the minikube tests, but does not start minikube; so you need to have started it with `minikube start` before.
 - `test-integration.sh`: Starts minikube, runs all tests, and stops minikube again.
 - `cargo run -p integration_test`: Starts a 'playground' based on the integration test, with all components running. As long as the process is running, there's a directory `playground/` in which the configuration, versions repo etc. live.
 - `cd ui && yarn install && yarn start`: Runs the ui. Currently expected to be used with the playground.
 - `cargo fmt`: This project uses rustfmt, so you should run this before committing.
