#!/bin/sh
set -e
cd "$(dirname "$0")"
source ./prep.sh
exec ./builda run.lua "$@"
