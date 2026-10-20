#!/usr/bin/env python3
"""Offline IndianOS application-route planner (host-side prototype).

This module does NOT install, download, execute, or verify applications.
It only explains which planned compatibility routes might apply.
Python 3.10+; standard library only.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

ROUTE_PREFERENCE = (
    "native",
    "web",
    "linux_compat",
    "windows_compat",
    "windows_vm",
    "android_runtime",
    "remote_mac",
)
ROUTES = frozenset(ROUTE_PREFERENCE)
ROUTE_STATES = frozenset(("ready", "planned", "disabled"))
STATUS_ORDER = {
    "verified": 0,
    "candidate": 1,
    "planned": 2,
    "unavailable": 3,
    "blocked": 4,
}


def load_json(path: str) -> dict[str, Any]:
    """Read a local, JSON-object manifest. No network or code execution."""
    parsed = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(parsed, dict):
        raise ValueError("manifest root must be a JSON object")
    return parsed


def plan_app(catalog: dict[str, Any], device: dict[str, Any],
             app_name: str) -> dict[str, Any]:
    """Provide explanations, never an executable installation decision.

    'verified' requires an app-option flag from real compatibility testing
    AND an available runtime. Sample data contains no verified options.
    """
    if not isinstance(app_name, str) or not app_name.strip():
        raise ValueError("app_name must be nonempty text")

    apps = catalog.get("apps", [])
    routes = device.get("routes", {})
    cpu_arch = device.get("cpu_arch")
    if not isinstance(apps, list) or not isinstance(routes, dict):
        raise ValueError("apps must be a list and device.routes an object")
    if not isinstance(cpu_arch, str) or not cpu_arch:
        raise ValueError("device.cpu_arch must be a nonempty string")
    for state in routes.values():
        if state not in ROUTE_STATES:
            raise ValueError("invalid route state in device profile")

    query = app_name.strip().casefold()
    match = next(
        (
            app for app in apps
            if isinstance(app, dict)
            and query in [
                alias.casefold()
                for alias in [app.get("name"), *app.get("aliases", [])]
                if isinstance(alias, str)
            ]
        ),
        None,
    )
    if match is None:
        return {
            "app": app_name.strip(),
            "status": "unknown",
            "choices": [],
            "installable": False,
            "message": "No trusted catalog entry; do not guess a download URL.",
        }

    options = match.get("options", [])
    if not isinstance(options, list):
        raise ValueError("app.options must be an array")
    choices = []
    for option in options:
        if not isinstance(option, dict):
            raise ValueError("app option must be an object")
        route = option.get("route")
        if route not in ROUTES:
            raise ValueError("unrecognized route in catalog")
        architectures = option.get("architectures", [cpu_arch])
        if not isinstance(architectures, list) or not all(
            isinstance(a, str) for a in architectures
        ):
            raise ValueError("architectures must be a string list")

        if cpu_arch not in architectures:
            status, reason = "blocked", "CPU architecture not supported by this option"
        elif route == "remote_mac" and not device.get("authorized_mac", False):
            status, reason = "blocked", "An authorized Mac host is required"
        elif route not in routes or routes[route] == "disabled":
            status, reason = "unavailable", "Runtime provider is unavailable"
        elif routes[route] == "planned":
            status, reason = "planned", "Runtime provider has not been implemented"
        elif option.get("verified_on_indianos") is True:
            status, reason = "verified", "Route and app verified on a defined test profile"
        else:
            status, reason = "candidate", "Runtime exists but this app needs verification"

        choices.append({
            "route": route,
            "status": status,
            "reason": reason,
            "user_approval_required": True,
        })

    choices.sort(key=lambda c: (
        STATUS_ORDER[c["status"]],
        ROUTE_PREFERENCE.index(c["route"]),
    ))
    return {
        "app": match.get("name"),
        "status": "plan_only",
        "choices": choices,
        "installable": False,
        "message": "Informational plan only; no package is downloaded or installed.",
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Offline IndianOS compatibility-route evaluation"
    )
    parser.add_argument("--catalog", required=True, help="Local app catalog JSON")
    parser.add_argument("--device", required=True, help="Local device profile JSON")
    parser.add_argument("--app", required=True, help="Exact catalog app name or alias")
    args = parser.parse_args()
    try:
        result = plan_app(load_json(args.catalog), load_json(args.device), args.app)
    except (ValueError, OSError, json.JSONDecodeError) as exc:
        parser.error(str(exc))
    print(json.dumps(result, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
