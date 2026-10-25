#!/usr/bin/env bash
# Kept for the install.sh of 0.7.0 and 0.8.0, which `smart-commit update`
# runs: after installing a release it calls this script to bring llama.cpp to
# that release's build. The binary does it now.
#
#   scripts/setup-llamacpp.sh [--server-only]
exec "$(dirname "$0")/../../../bin/smart-commit" setup --llamacpp-only
