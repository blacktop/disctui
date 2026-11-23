set shell := ["bash", "-euo", "pipefail", "-c"]
set positional-arguments

# List available commands.
default:
    @just --list

# Build the release binary with Discord support.
build:
    cargo build --release --locked --features experimental-discord
    target/release/disctui --version

# Run the TUI, or a subcommand such as `just run digest --since 12h`.
run *args:
    cargo run --features experimental-discord -- "$@"

# Build and install the release binary into ~/.cargo/bin.
install: build
    install -d "$HOME/.cargo/bin"
    install -m 755 target/release/disctui "$HOME/.cargo/bin/"

# Install the binary, then add its Rex command palette actions to Rex.
install-rex: install
    DISCTUI_BIN="$HOME/.cargo/bin/disctui" rex/install.sh

# Check formatting, lints, and tests with and without Discord support.
check:
    cargo fmt --check
    cargo clippy --all-targets --all-features --locked -- -D warnings
    cargo clippy --all-targets --locked -- -D warnings
    cargo test --all-features --locked -- --quiet
    cargo test --locked -- --quiet

# Follow the debug log written by `disctui --debug`.
logs:
    tail -f "$HOME/Library/Application Support/disctui/disctui.log"

# Validate the workflows and GoReleaser configuration.
release-check:
    just --fmt --check
    actionlint .github/workflows/*.yml
    zizmor --offline .github/workflows/
    goreleaser check

# Build local release archives without publishing them.
package:
    goreleaser release --snapshot --clean

# Publish the checked-out tag; called by GitHub Actions.
[private]
release-publish:
    goreleaser release --clean

[private]
check-version version:
    #!/usr/bin/env bash
    set -euo pipefail
    dt_tag="$1"
    if [[ ! "$dt_tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
        printf 'Invalid release tag %q; expected vMAJOR.MINOR.PATCH\n' "$dt_tag" >&2
        exit 1
    fi
    dt_version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')
    if [[ "${dt_tag#v}" != "$dt_version" ]]; then
        printf 'Tag %s does not match Cargo.toml version %s\n' "$dt_tag" "$dt_version" >&2
        exit 1
    fi

# Enforce release provenance in GitHub Actions, independently of the tag command.
[private]
verify-release-tag version:
    #!/usr/bin/env bash
    set -euo pipefail
    dt_tag="$1"
    just check-version "$dt_tag"
    dt_ref="refs/tags/$dt_tag"
    if [[ "$(git cat-file -t "$dt_ref")" != tag ]]; then
        printf 'Release tag %s must be annotated and signed\n' "$dt_tag" >&2
        exit 1
    fi
    dt_object=$(git rev-parse --verify "$dt_ref")
    dt_commit=$(git rev-parse --verify "$dt_ref^{commit}")
    if [[ "$dt_commit" != "$(git rev-parse --verify 'HEAD^{commit}')" ]]; then
        printf 'Release checkout does not match tag %s\n' "$dt_tag" >&2
        exit 1
    fi
    dt_verified=$(gh api "repos/blacktop/disctui/git/tags/$dt_object" --jq '.verification.verified')
    if [[ "$dt_verified" != true ]]; then
        printf 'GitHub has not verified the signature on tag %s\n' "$dt_tag" >&2
        exit 1
    fi
    git fetch --no-tags origin '+refs/heads/main:refs/remotes/origin/main'
    if ! git merge-base --is-ancestor "$dt_commit" refs/remotes/origin/main; then
        printf 'Release tag %s is not in origin/main history\n' "$dt_tag" >&2
        exit 1
    fi

# Check, sign, and push a stable release tag, for example: just release v0.2.0.
release version:
    #!/usr/bin/env bash
    set -euo pipefail
    dt_tag="$1"
    just check-version "$dt_tag"
    dt_head=$(git rev-parse --verify 'HEAD^{commit}')
    dt_ref="refs/tags/$dt_tag"
    dt_require_main() {
        local dt_branch dt_current dt_status dt_remote
        dt_branch=$(git branch --show-current)
        dt_current=$(git rev-parse --verify 'HEAD^{commit}')
        dt_status=$(git status --porcelain)
        if [[ "$dt_branch" != main || "$dt_current" != "$dt_head" || -n "$dt_status" ]]; then
            printf 'Release requires a clean main checkout at %s\n' "$dt_head" >&2
            exit 1
        fi
        dt_remote=$(git ls-remote --exit-code origin refs/heads/main)
        if [[ "$dt_remote" != "$dt_head"$'\trefs/heads/main' ]]; then
            printf 'Push main before releasing; origin/main must match %s\n' "$dt_head" >&2
            exit 1
        fi
    }
    dt_require_main
    if git show-ref --verify --quiet "$dt_ref"; then
        printf 'Tag %s already exists locally; inspect it before retrying a push\n' "$dt_tag" >&2
        exit 1
    fi
    dt_remote_tag=$(git ls-remote --tags origin "$dt_ref")
    if [[ -n "$dt_remote_tag" ]]; then
        printf 'Tag %s already exists on origin\n' "$dt_tag" >&2
        exit 1
    fi
    just check
    just build
    just release-check
    dt_require_main
    git tag -s "$dt_tag" "$dt_head" -m "Release $dt_tag"
    if ! git push --no-follow-tags origin "$dt_ref"; then
        printf 'Tag %s remains local. After resolving the push failure, retry: git push --no-follow-tags origin %s\n' "$dt_tag" "$dt_ref" >&2
        exit 1
    fi
    printf 'Pushed %s; GitHub Actions will publish the release and Homebrew cask.\n' "$dt_tag"

# Bump the version (patch, minor, or major), commit, push, and release it: just bump minor.
bump level="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    dt_level="$1"
    if [[ "$(git branch --show-current)" != main || -n "$(git status --porcelain)" ]]; then
        printf 'Bump requires a clean main checkout\n' >&2
        exit 1
    fi
    dt_current=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')
    if [[ ! "$dt_current" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
        printf 'Cannot bump %q; expected MAJOR.MINOR.PATCH in Cargo.toml\n' "$dt_current" >&2
        exit 1
    fi
    dt_major=${BASH_REMATCH[1]} dt_minor=${BASH_REMATCH[2]} dt_patch=${BASH_REMATCH[3]}
    case "$dt_level" in
        patch) dt_patch=$((dt_patch + 1)) ;;
        minor) dt_minor=$((dt_minor + 1)) dt_patch=0 ;;
        major) dt_major=$((dt_major + 1)) dt_minor=0 dt_patch=0 ;;
        *)
            printf 'Bump level must be patch, minor, or major, not %q\n' "$dt_level" >&2
            exit 1
            ;;
    esac
    dt_next="$dt_major.$dt_minor.$dt_patch"
    dt_ref="refs/tags/v$dt_next"
    if git show-ref --verify --quiet "$dt_ref" || [[ -n "$(git ls-remote --tags origin "$dt_ref")" ]]; then
        printf 'Tag v%s already exists\n' "$dt_next" >&2
        exit 1
    fi
    # Fail before anything is committed or pushed.
    just check
    dt_current="$dt_current" dt_next="$dt_next" perl -pi -e \
        '$done ||= s/^version = "\Q$ENV{dt_current}\E"$/version = "$ENV{dt_next}"/' Cargo.toml
    cargo update --workspace
    just check-version "v$dt_next"
    git commit --quiet -m "chore: Release disctui version $dt_next" Cargo.toml Cargo.lock
    git push origin main
    just release "v$dt_next"
