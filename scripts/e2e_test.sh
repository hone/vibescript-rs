#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

echo "Building VibeScript CLI and Core..."
cargo build -q -p vibescript-cli --release

VIBES_BIN="target/release/vibescript-cli"

echo "Starting VibeScript Rust E2E Tests..."
echo "------------------------------------"

# Find all .vibe files in examples
EXAMPLES=$(find examples -name "*.vibe" | sort)

FAILED=0
PASSED=0

START_TIME=$(date +%s%N)

for example in $EXAMPLES; do
    echo -n "Testing $example... "
    
    # 1. Always do a syntax check
    if $VIBES_BIN run "$example" --check > /dev/null 2>&1; then
        # 2. If it has a 'run' function, try to execute it
        if grep -q "def run" "$example"; then
            if $VIBES_BIN run "$example" --function run > /dev/null 2>&1; then
                echo -e "${GREEN}PASSED (run)${NC}"
                PASSED=$((PASSED+1))
            else
                echo -e "${RED}FAILED (execution)${NC}"
                FAILED=$((FAILED+1))
            fi
        else
            echo -e "${GREEN}PASSED (check)${NC}"
            PASSED=$((PASSED+1))
        fi
    else
        echo -e "${RED}FAILED (syntax)${NC}"
        FAILED=$((FAILED+1))
    fi
done

END_TIME=$(date +%s%N)
DURATION=$(( (END_TIME - START_TIME) / 1000000 ))

echo "------------------------------------"
echo -e "E2E Results: ${GREEN}$PASSED passed${NC}, ${RED}$FAILED failed${NC}"
echo "Total Duration: ${DURATION}ms"

if [ $FAILED -ne 0 ]; then
    exit 1
fi
