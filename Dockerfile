FROM alpine AS build
RUN apk add bash cmake curl gcc git musl-dev ninja --no-cache
RUN curl -s https://mise.run | MISE_INSTALL_PATH=/bin/mise sh
ADD .tool-versions /app/.tool-versions
WORKDIR /app
RUN mise install --yes
ADD . /app
RUN PATH=$PATH:~/.local/share/mise/shims /app/build.sh lin-a64 lin-x64 win-a64 win-x64

FROM alpine
COPY --from=build /app/bin/* /usr/local/bin/
