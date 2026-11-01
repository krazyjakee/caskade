#!/bin/sh
set -eu
cd -- "$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
exec ./FixtureGame.x86_64 "$@"
