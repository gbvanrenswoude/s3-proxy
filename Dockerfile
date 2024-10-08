FROM rust:1.72-slim AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -f src/main.rs
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/s3-proxy /app/s3-proxy

EXPOSE 8090

ENV S3_URL=https://s3-bucket-as-loginpage.ds-fdn-d.aws.insim.biz.s3.eu-west-1.amazonaws.com

CMD ["./s3-proxy"]