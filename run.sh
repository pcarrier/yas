#!/bin/sh
cd "$(dirname $0)"
set -eux
go run . "$@"
