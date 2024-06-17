FROM alpine
RUN apk add --no-cache go zig \
  --repository=https://dl-cdn.alpinelinux.org/alpine/edge/main \
  --repository=https://dl-cdn.alpinelinux.org/alpine/edge/community
ADD . /app
RUN /app/build.sh
