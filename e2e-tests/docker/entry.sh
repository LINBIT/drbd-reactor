#!/bin/bash

set -e

# Read TARGETS into array targets without messing up IFS
IFS=, read -a targets <<< "$TARGETS"

cd /virter/workspace/
./drbd_reactor_test.py --test "$TEST_NAME" "${targets[@]}"
