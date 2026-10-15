#!/usr/bin/env bash
set -euo pipefail
cd /home/codingmachine/Development/ai-brain
exec php artisan mcp:start brain
