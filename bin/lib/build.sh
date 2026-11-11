# shellcheck shell=sh
#
# Builds the optimized CLI for the bin/ scripts. The caller sets CADPOLE_ROOT.
# All of Bazel's output goes to stderr.

CADPOLE_TARGET=//src/rust/cadpole-cli:cadpole
# `--config=opt` keeps its own `bazel-opt-bin` symlink (.bazelrc), which no
# test run repoints at a debug build.
CADPOLE_BIN=$CADPOLE_ROOT/bazel-opt-bin/src/rust/cadpole-cli/cadpole

build_cadpole() {
    # A quiet check first, so an up-to-date binary costs one Bazel round trip
    # and prints nothing; only a real build shows Bazel's progress.
    if [ -x "$CADPOLE_BIN" ] && (cd "$CADPOLE_ROOT" &&
        bazel build --config=opt --check_up_to_date --noshow_progress "$CADPOLE_TARGET") >/dev/null 2>&1; then
        return 0
    fi
    echo "cadpole: building $CADPOLE_TARGET (the first build takes a while)" >&2
    if ! (cd "$CADPOLE_ROOT" && bazel build --config=opt "$CADPOLE_TARGET") 1>&2; then
        echo "cadpole: the build failed" >&2
        return 1
    fi
}
