FROM rust:1.81-slim as builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM ubuntu

WORKDIR /app
EXPOSE 3000

RUN apt update; apt upgrade; apt install -y ffmpeg

COPY --from=builder /build/target/release/handler .

CMD  [ "./handler" ] 
