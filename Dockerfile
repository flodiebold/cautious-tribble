# backend
FROM flodiebold/cautious-tribble-build
WORKDIR /home/rust/src
USER rust
COPY . ./
RUN cargo build -p deployer -p transitioner -p aggregator --release

# ui
FROM node:11-alpine
WORKDIR /home/node/src
RUN chown node .
USER node
WORKDIR /home/node/src
COPY --chown=node ./ui ./
RUN yarn install && yarn build

FROM scratch
COPY --from=0 /home/rust/src/target/x86_64-unknown-linux-musl/release/deployer /home/rust/src/target/x86_64-unknown-linux-musl/release/transitioner /home/rust/src/target/x86_64-unknown-linux-musl/release/aggregator /bin/
COPY --from=1 /home/node/src/dist /ui/dist/
COPY --from=lachlanevenson/k8s-kubectl:v1.10.3 /usr/local/bin/kubectl /bin/
