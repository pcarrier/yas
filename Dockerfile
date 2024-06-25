FROM alpine AS build
RUN apk add bash cmake curl gcc git musl-dev ninja zig --no-cache
ADD . /app
RUN /app/build.sh linux-aarch64 linux-x86_64 windows-aarch64 windows-x86_64

FROM alpine
COPY --from=build /app/bin/yas-* /usr/local/bin/
