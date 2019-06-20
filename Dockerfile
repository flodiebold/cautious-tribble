# backend
FROM flodiebold/cautious-tribble-build
WORKDIR /src
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

FROM alpine@sha256:ca1c944a4f8486a153024d9965aafbe24f5723c1d5c02f4964c045a16d19dc54
RUN apk add --no-cache libstdc++ openssl
COPY --from=0 /src/target/release/deployer /src/target/release/transitioner /src/target/release/aggregator /bin/
COPY --from=1 /home/node/src/dist /ui/dist/
COPY --from=lachlanevenson/k8s-kubectl:v1.10.3 /usr/local/bin/kubectl /bin/
