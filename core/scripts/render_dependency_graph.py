#!/usr/bin/env python3
"""Render a more readable Graphviz DOT from cargo-modules output."""

from __future__ import annotations

import argparse
import re
from pathlib import Path


NODE_RE = re.compile(
    r'^\s*"(?P<id>[^"]+)" \[label="(?P<label>[^"]+)", fillcolor="(?P<fill>#[0-9a-fA-F]+)"\];'
)
EDGE_RE = re.compile(
    r'^\s*"(?P<src>[^"]+)" -> "(?P<dst>[^"]+)".*style="(?P<style>[^"]+)".*\[constraint=(?P<constraint>\w+)\];'
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, help="Input DOT from cargo-modules")
    parser.add_argument("--output", required=True, help="Output DOT path")
    parser.add_argument(
        "--mode",
        choices=("full", "application"),
        default="full",
        help="Grouping strategy for the graph",
    )
    return parser.parse_args()


def short_label(node_id: str) -> str:
    prefix = "tsuki_core_rust::"
    if node_id == "tsuki_core_rust":
        return "crate root"
    if node_id.startswith(prefix):
        node_id = node_id[len(prefix) :]
    return node_id.replace("application::", "").replace("commands::", "commands/")


def group_for(node_id: str, mode: str) -> str:
    if node_id == "tsuki_core_rust":
        return "entry"
    if node_id.startswith("tsuki_core_rust::application::"):
        return "application"
    if mode == "application":
        return "root"
    if node_id in {
        "tsuki_core_rust::server_app",
        "tsuki_core_rust::cli",
        "tsuki_core_rust::commands::backfill",
    }:
        return "entry"
    if node_id in {
        "tsuki_core_rust::config",
        "tsuki_core_rust::clock",
        "tsuki_core_rust::event",
        "tsuki_core_rust::event::contracts",
        "tsuki_core_rust::prompts",
        "tsuki_core_rust::module_registry",
    }:
        return "core"
    if node_id in {
        "tsuki_core_rust::db",
        "tsuki_core_rust::event_store",
        "tsuki_core_rust::state",
        "tsuki_core_rust::scheduler",
        "tsuki_core_rust::notification",
    }:
        return "storage"
    return "integration"


GROUP_STYLES = {
    "entry": ("Entry", "#d9ecff"),
    "application": ("Application", "#dcf5e4"),
    "core": ("Core", "#fff2cc"),
    "storage": ("Storage", "#fde2d2"),
    "integration": ("Integration", "#efe3ff"),
    "root": ("Root", "#d9ecff"),
}


GROUP_ORDER_FULL = ["entry", "application", "core", "storage", "integration"]
GROUP_ORDER_APPLICATION = ["root", "application"]


def main() -> int:
    args = parse_args()
    src = Path(args.input).read_text()

    nodes: dict[str, tuple[str, str]] = {}
    edges: list[tuple[str, str, str, str]] = []
    for line in src.splitlines():
        node_match = NODE_RE.match(line)
        if node_match:
            nodes[node_match.group("id")] = (
                node_match.group("label"),
                node_match.group("fill"),
            )
            continue
        edge_match = EDGE_RE.match(line)
        if edge_match:
            edges.append(
                (
                    edge_match.group("src"),
                    edge_match.group("dst"),
                    edge_match.group("style"),
                    edge_match.group("constraint"),
                )
            )

    groups: dict[str, list[str]] = {}
    for node_id in nodes:
        groups.setdefault(group_for(node_id, args.mode), []).append(node_id)

    group_order = GROUP_ORDER_APPLICATION if args.mode == "application" else GROUP_ORDER_FULL
    lines: list[str] = [
        "digraph {",
        '    graph [rankdir=LR, newrank=true, concentrate=true, ranksep=1.0, nodesep=0.4, pad=0.3, fontname="Helvetica", fontsize="24", label="core-rust internal dependencies", labelloc=t];',
        '    node [shape=box, style="rounded,filled", fontname="Helvetica", fontsize="11", color="#7a7a7a", penwidth=1.0, margin="0.10,0.06"];',
        '    edge [color="#7f8c8d", arrowsize=0.7, penwidth=1.0, fontname="Helvetica", fontsize="9"];',
        "",
    ]

    for group in group_order:
        node_ids = sorted(groups.get(group, []))
        if not node_ids:
            continue
        title, color = GROUP_STYLES[group]
        lines.append(f"    subgraph cluster_{group} {{")
        lines.append(f'        label="{title}";')
        lines.append('        color="#d0d7de";')
        lines.append('        penwidth=1.0;')
        lines.append('        style="rounded";')
        lines.append("        rank=same;")
        for node_id in node_ids:
            label = short_label(node_id).replace('"', '\\"')
            lines.append(
                f'        "{node_id}" [label="{label}", fillcolor="{color}"];'
            )
        lines.append("    }")
        lines.append("")

    # Encourage left-to-right layering between groups without dropping any nodes.
    representative: list[str] = []
    for group in group_order:
        node_ids = sorted(groups.get(group, []))
        if node_ids:
            representative.append(node_ids[0])
    if len(representative) > 1:
        for left, right in zip(representative, representative[1:]):
            lines.append(f'    "{left}" -> "{right}" [style=invis, weight=20];')
        lines.append("")

    for src_id, dst_id, style, constraint in edges:
        if src_id not in nodes or dst_id not in nodes:
            continue
        edge_style = "dashed" if style == "dashed" else "solid"
        lines.append(
            f'    "{src_id}" -> "{dst_id}" [style="{edge_style}", constraint={constraint}];'
        )

    lines.append("}")
    Path(args.output).write_text("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
