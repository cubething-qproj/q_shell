# ------------------------------------------
# SPDX-License-Identifier: MIT OR Apache-2.0
# -------------------------------- 𝒒𝒑𝒓𝒐𝒋 --

QPROJ_REF := env("QPROJ_SCRIPTS_REF", "main")
QPROJ_GIT_URL := "git+https://github.com/cubething-qproj/infra.git@" + QPROJ_REF + "#subdirectory=scripts"
SCRIPTS_SRC := env("QPROJ_SCRIPTS_SRC", QPROJ_GIT_URL)
qproj := "qproj-scripts"
NIXGL := env("NIXGL", "nixVulkanNvidia")

_default:
    just --list

# Install the shared CLI.
sync-scripts:
    uv tool install qproj-scripts --from {{ SCRIPTS_SRC }}

# Build the workspace.
[working-directory: '.']
build *args:
    {{ qproj }} build {{ args }}

# Run the application.
[working-directory: '.']
play *args:
    nix run --impure github:nix-community/nixGL#{{ NIXGL }} -- \
        {{ qproj }} play {{ args }}

# Lint with Clippy and bevy_lint.
[working-directory: '.']
check *args:
    {{ qproj }} check {{ args }}

# Run clippy.
[working-directory: '.']
clippy *args:
    {{ qproj }} clippy {{ args }}

# Run bevy_lint.
[working-directory: '.']
bevy-lint *args:
    {{ qproj }} bevy-lint {{ args }}

# Check dependencies with cargo-deny.
[working-directory: '.']
deny:
    {{ qproj }} deny

# Run tests via cargo-nextest.
[working-directory: '.']
test *args:
    {{ qproj }} test {{ args }}

# Generate test coverage report.
[working-directory: '.']
coverage *args:
    {{ qproj }} coverage {{ args }}

# Fix all fixable issues.
[working-directory: '.']
fix *args:
    {{ qproj }} fix {{ args }}

# Test CI locally with act.
[working-directory: '.']
ci *args:
    {{ qproj }} ci {{ args }}

# Emit Clippy + bevy_lint diagnostics as JSON for rust-analyzer.
[working-directory: '.']
ra-check *args:
    {{ qproj }} ra-check {{ args }}
