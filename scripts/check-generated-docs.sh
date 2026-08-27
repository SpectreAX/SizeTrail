#!/bin/bash
set -euo pipefail

git diff --exit-code -- docs/generated
