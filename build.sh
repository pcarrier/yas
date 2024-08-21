#!/bin/sh
cd "$(dirname $0)"
set -eux
GOOS=darwin  GOARCH=amd64 go build -ldflags '-s -w' -o bin/yas-darwin-amd64 .
GOOS=darwin  GOARCH=arm64 go build -ldflags '-s -w' -o bin/yas-darwin-arm64 .
GOOS=linux   GOARCH=amd64 go build -ldflags '-s -w' -o bin/yas-linux-amd64 .
GOOS=linux   GOARCH=arm64 go build -ldflags '-s -w' -o bin/yas-linux-arm64 .
GOOS=windows GOARCH=amd64 go build -ldflags '-s -w' -o bin/yas-window-amd64.exe .
GOOS=windows GOARCH=arm64 go build -ldflags '-s -w' -o bin/yas-window-amd64.exe .
