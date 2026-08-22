FROM rust:bookworm AS wasm

RUN rustup target add wasm32-unknown-unknown \
    && cargo install wasm-pack --version 0.15.0 --locked
WORKDIR /src
COPY . .
RUN wasm-pack build crates/browser --target web --release --out-dir pkg

FROM node:24-bookworm AS web

RUN npm install --global pnpm@11.21.0
WORKDIR /src
COPY js ./js
COPY --from=wasm /src/crates/browser/pkg ./crates/browser/pkg
RUN cd js \
    && pnpm install --frozen-lockfile \
    && pnpm --filter yas-web typecheck \
    && pnpm --filter yas-web build

FROM rust:bookworm AS server

WORKDIR /src
COPY . .
COPY --from=web /src/js/web/dist ./js/web/dist
RUN cargo build --locked --release -p yas-website \
    && strip target/release/yas-website

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=server /src/target/release/yas-website /usr/local/bin/yas-website
ENV YAS_WEBSITE_ADDR=0.0.0.0:8000
EXPOSE 8000
ENTRYPOINT ["yas-website"]
