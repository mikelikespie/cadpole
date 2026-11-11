"""`quint_test`: Quint models typechecked, simulated and scenario-tested as a
Bazel test.

    load("//build_support/quint:defs.bzl", "quint_test")

    quint_test(
        name = "quint_test",
        models = {
            "session.qnt": ["seqMonotonic", "deliveryIsPrefix"],
            "jobs.qnt": ["displayIsNewest"],
        },
    )

What the macro guarantees:

  * quint is the pinned `@informalsystems/quint` from the pnpm workspace
    (`spec/package.json` + `//:pnpm-lock.yaml`), never `pnpm dlx`, so its
    absence is a build error rather than a silent network fetch;
  * its Rust simulation backend comes from
    //build_support/quint:evaluator -- sha256-pinned, fetched by Bazel -- and
    is reached through `QUINT_HOME` inside runfiles, so quint never downloads
    one from GitHub at test time;
  * the test carries `block-network`, so if that ever stopped being true the
    test would fail instead of quietly reaching the network;
  * every `run` definition in each model is executed. `quint test` with no
    `--match` selects only names ending in "Test", i.e. none of ours, and then
    reports success having run nothing; runner.mjs builds the match expression
    from the file.

A companion `<name>_check` binary (`bazel run`) runs the same checks with the
knobs exposed, for a deeper pass by hand:

    bazel run //spec:quint_check -- --samples=20000 --steps=60
"""

load("@aspect_rules_js//js:defs.bzl", "js_binary", "js_test")

_RUNNER = "//build_support/quint:runner"
# Imported by runner.mjs; data, because an entry point's deps are not runfiles.
_LINT = "//build_support/quint:precedence_lint"
_EVALUATOR = "//build_support/quint:evaluator"
_MARKER = "//build_support/quint:quint_home_marker"

# `rustEvaluatorDir()` in the pinned quint's dist/src/config.js.
_EVALUATOR_DIR = "rust-evaluator-v0.6.0"

def _check_name(name):
    return (name[:-len("_test")] if name.endswith("_test") else name) + "_check"

def quint_test(
        name,
        models,
        quint = ":node_modules/@informalsystems/quint",
        mutants = {},
        samples = 2000,
        steps = 40,
        data = [],
        tags = [],
        timeout = "moderate",
        **kwargs):
    """Typechecks, simulates and scenario-tests Quint models.

    Args:
        name: test target name.
        models: {model file (package-relative label) -> [invariant names]}. An
            empty invariant list still typechecks the model and runs its
            scenarios.
        mutants: {model file -> {init action -> invariant}}. Each init action
            starts the model in a deliberately broken variant; the simulation
            from it must violate the invariant, or the test fails.
        quint: the linked `@informalsystems/quint` npm package.
        samples: default `--max-samples` for the random simulation. What this
            test costs is node process start-up, not simulation (each run takes
            tens of milliseconds), so the default is generous.
        steps: default `--max-steps`.
        data: extra runfiles (models `import`ed by the ones listed, say).
        tags: extra tags.
        timeout: Bazel test timeout.
        **kwargs: forwarded to `js_test` / `js_binary`.
    """
    model_files = sorted(models.keys())
    plan = {
        "samples": samples,
        "steps": steps,
        "models": [
            {
                "file": "$(rlocationpath %s)" % f,
                "invariants": models[f],
                "mutants": sorted(mutants.get(f, {}).items()),
            }
            for f in model_files
        ],
    }

    # `$(rlocationpath …)` is expanded by Bazel inside the env value; encoding
    # the plan as JSON keeps one variable instead of one per model.
    env = {
        "QUINT_PLAN": json.encode(plan),
        "QUINT_HOME_MARKER": "$(rlocationpath {})".format(_MARKER),
        "QUINT_EVALUATOR_DIR": _EVALUATOR_DIR,
        "QUINT_CLI": "node_modules/@informalsystems/quint/dist/src/cli.js",
    }

    all_data = depset(data + model_files + [quint, _EVALUATOR, _MARKER, _LINT]).to_list()

    js_test(
        name = name,
        chdir = native.package_name(),
        data = all_data,
        entry_point = _RUNNER,
        env = env,
        no_copy_to_bin = [_EVALUATOR, _MARKER],
        # Nothing here may reach the network: the point of pinning quint and
        # its evaluator is that a test never fetches either one.
        tags = tags + ["block-network"],
        timeout = timeout,
        **kwargs
    )

    js_binary(
        name = _check_name(name),
        chdir = native.package_name(),
        data = all_data,
        entry_point = _RUNNER,
        env = env,
        no_copy_to_bin = [_EVALUATOR, _MARKER],
        tags = tags + ["manual"],
        **kwargs
    )

def quint_replay(
        name,
        entry_point,
        models,
        data = [],
        quint = "//spec:node_modules/@informalsystems/quint",
        tags = [],
        timeout = "moderate",
        **kwargs):
    """A replay of a model's traces against the implementation.

    The entry point generates traces with the pinned quint (spec/replay/
    traces.mjs reads the environment set here) and drives the implementation
    through them. Defines `<name>_test`, a bounded run that is part of the
    test sweep, and `<name>`, the same program for `bazel run` with
    `--traces=N --steps=N --seed=0x...`.

    Args:
        name: base name of the two targets.
        entry_point: the replay program.
        models: labels of the model files it reads.
        data: what it drives: the implementation and its stand-ins.
        quint: the linked `@informalsystems/quint` npm package.
        tags: extra tags.
        timeout: Bazel test timeout of `<name>_test`.
        **kwargs: forwarded to `js_test` / `js_binary`.
    """
    env = {
        # The package is a directory of many files, which `$(rlocationpath)`
        # cannot name; it is linked at its label's path under the workspace.
        "QUINT_PACKAGE": "_main/" + quint.lstrip("/").replace(":", "/"),
        "QUINT_HOME_MARKER": "$(rlocationpath {})".format(_MARKER),
    }
    all_data = depset(data + models + [quint, _EVALUATOR, _MARKER]).to_list()
    no_copy = models + [_EVALUATOR, _MARKER]

    js_test(
        name = name + "_test",
        data = all_data,
        entry_point = entry_point,
        env = env,
        no_copy_to_bin = no_copy,
        # Loopback only: the implementation and its stand-ins talk to each
        # other on this machine and to nothing else.
        tags = tags + ["block-network"],
        timeout = timeout,
        **kwargs
    )

    js_binary(
        name = name,
        data = all_data,
        entry_point = entry_point,
        env = env,
        no_copy_to_bin = no_copy,
        tags = tags + ["manual"],
        **kwargs
    )
