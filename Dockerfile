FROM rust:latest

WORKDIR /app

COPY . .

RUN cargo build --release
RUN cargo install --path .

CMD ["/app/target/release/www-bot"]
