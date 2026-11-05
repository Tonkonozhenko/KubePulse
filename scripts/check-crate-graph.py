#!/usr/bin/env python3
"""Fail if workspace crates break the dependency rules in design decision D1."""

import json
import subprocess
import sys

CLUSTER = "kubepulse-cluster"
STORE = "kubepulse-store"
API = "kubepulse-extension-api"
HOST = "kubepulse-extension-host"
BUILTIN = "kubepulse-builtin"
UI = "kubepulse-ui"
BINARY = "kubepulse"

# The workspace crates each crate may depend on. A crate missing from this
# table fails the check, so new crates have to choose their place in the graph.
ALLOWED = {
    BINARY: {UI, BUILTIN, HOST, API, STORE, CLUSTER},
    UI: {HOST, API, STORE, CLUSTER},
    BUILTIN: {API},
    HOST: {API, STORE, CLUSTER},
    API: set(),
    STORE: {CLUSTER},
    CLUSTER: set(),
}

# External crates confined to one workspace crate, by name prefix.
CONFINED = (
    ("gpui", UI, "the UI toolkit"),
    ("kube", CLUSTER, "the Kubernetes client"),
    ("k8s-openapi", CLUSTER, "the Kubernetes client"),
)


def violations(packages):
    workspace = {package["name"] for package in packages}
    for name in sorted(workspace - ALLOWED.keys()):
        yield f"{name}: not listed in the allowed dependency table"

    for package in packages:
        name = package["name"]
        # Includes dev and build dependencies, which can bypass the graph too.
        for dependency in package["dependencies"]:
            target = dependency["name"]
            if target in workspace:
                if target not in ALLOWED.get(name, set()):
                    yield f"{name}: must not depend on {target}"
                continue
            for prefix, owner, description in CONFINED:
                if target.startswith(prefix) and name != owner:
                    yield f"{name}: must not depend on {target}, {description} belongs to {owner}"


def main():
    output = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    found = sorted(set(violations(json.loads(output)["packages"])))
    for line in found:
        print(line, file=sys.stderr)
    if found:
        return 1
    print("crate graph ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
