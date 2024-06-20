FROM alpine AS build
RUN apk add bash cmake curl gcc git musl-dev nim ninja zig --repository=https://dl-cdn.alpinelinux.org/alpine/edge/main --repository=https://dl-cdn.alpinelinux.org/alpine/edge/community --no-cache
ADD . /app
RUN /app/build.sh lin-a64 lin-x64 win-a64 win-x64

FROM alpine
COPY --from=build /app/bin/yas-* /usr/local/bin/
