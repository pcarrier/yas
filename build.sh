#!/bin/sh
set -e
cd "$(dirname "$0")"
. ./prep.sh
exec ./bin/builda build.lua "$@"
