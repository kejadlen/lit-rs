default: all

# Tangle the literate sources in lit/ into src/, then format.
tangle:
    cargo run --quiet -- lit .
    cargo fmt --all

fmt:
    cargo fmt --all

check: tangle
    cargo check --workspace

clippy: tangle
    cargo clippy --workspace --all-targets -- -D warnings

coverage: tangle
    #!/usr/bin/env bash
    set -euo pipefail
    export RUSTFLAGS="-Cinstrument-coverage"
    export CARGO_TARGET_DIR="target/coverage"
    export LLVM_PROFILE_FILE="target/coverage/profraw/%p-%m.profraw"
    rm -rf target/coverage
    cargo test --workspace -q
    REPORT=$(grcov target/coverage/profraw \
        --binary-path ./target/coverage/debug/ \
        -s . \
        -t covdir \
        --ignore-not-existing \
        --keep-only 'src/**' \
        --ignore 'src/main.rs' \
        --excl-line 'cov-excl-line|unreachable!' \
        --excl-start 'cov-excl-start' \
        --excl-stop 'cov-excl-stop')
    echo "$REPORT" | jq -r '
        def files:
            to_entries[] | .value |
            if .children then .children | files
            else "\(.name): \(.coveragePercent)% (\(.linesCovered)/\(.linesTotal))"
            end;
        .children | files
    '
    COVERAGE=$(echo "$REPORT" | jq '.coveragePercent')
    echo ""
    echo "Total: ${COVERAGE}%"
    if [ "$(echo "$COVERAGE < 100" | bc -l)" -eq 1 ]; then
        echo "ERROR: Coverage is below 100%"
        exit 1
    fi

mutants: tangle
    #!/usr/bin/env bash
    set -uo pipefail
    cargo mutants --timeout-multiplier 3 -j4
    rc=$?
    # 0 = all caught, 3 = timeouts (infinite loops from mutants, still caught).
    if [ "$rc" -eq 0 ] || [ "$rc" -eq 3 ]; then
        exit 0
    fi
    exit "$rc"

all: clippy coverage

install:
    cargo install --locked --path .

# Restore generated src/ from version control.
clean:
    jj restore src
