#!/bin/sh

set -e

cd "$(dirname $0)"

for OS in darwin linux windows; do
for ARCH in amd64 arm64; do
CGO_ENABLED=0 GOARCH=$ARCH GOOS=$OS go build -ldflags '-s -w' -o yas-$OS-$ARCH .
done
done
