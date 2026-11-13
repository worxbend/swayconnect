# swayconnect task runner. Agents call cargo through these recipes only.
#
# Every recipe takes one lock, so builds and test runs of parallel agents queue
# (docs/IMPLEMENTATION_PLAN.md 1.2). The job count is capped in .cargo/config.toml.

lock := "mkdir -p target && flock target/.agent.lock"

# Format all workspace crates.
fmt:
    {{ lock }} cargo fmt --all

# Check formatting without changing files.
fmt-check:
    {{ lock }} cargo fmt --all -- --check

# Type-check one crate, or the workspace when no crate is named.
check crate="":
    {{ lock }} cargo check {{ if crate == "" { "--workspace" } else { "-p " + crate } }} --all-targets

# Clippy with warnings as errors, for one crate or the workspace.
lint crate="":
    {{ lock }} cargo clippy {{ if crate == "" { "--workspace" } else { "-p " + crate } }} --all-targets -- -D warnings

# Tests of one crate, or of the workspace. Extra arguments go to cargo test.
# The CLI end-to-end tests start the daemon binary, which cargo builds only in a workspace run.
test crate="" *args:
    {{ if crate == "swayconnect-cli" { lock + " cargo build -p swayconnect-daemon --bin swayconnectd --locked" } else { "true" } }}
    {{ lock }} cargo test {{ if crate == "" { "--workspace" } else { "-p " + crate } }} --locked {{ args }}

# Licenses, advisories, bans, sources.
deny:
    {{ lock }} cargo deny check

# The full gate.
ci: fmt-check lint test deny
