FROM alpine@sha256:ca1c944a4f8486a153024d9965aafbe24f5723c1d5c02f4964c045a16d19dc54
RUN apk add --no-cache libgcc g++ openssl openssl-dev \
  && apk add --no-cache --virtual .build-rust rust cargo
WORKDIR /src
COPY . ./
RUN cargo build -p deployer -p transitioner -p aggregator --release
