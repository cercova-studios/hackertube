#!/usr/bin/env python3
"""Benchmark an explicit catalog snapshot. Never open the source for writes."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import platform
import random
import sqlite3
import statistics
import struct
import subprocess
import time


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)]


def distribution(values):
    return {"n": len(values), "p50": statistics.median(values),
            "p95": percentile(values, .95), "max": max(values)}


def backup(source, destination):
    if destination.exists():
        raise FileExistsError(destination)
    with sqlite3.connect(f"file:{source}?mode=ro", uri=True) as src:
        with sqlite3.connect(destination) as dst:
            src.backup(dst)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.mkdir(exist_ok=False)
    here = Path(__file__).resolve().parent
    for name, features in [("fork", ["--features", "fork"]),
                           ("upstream", ["--no-default-features", "--features", "upstream"])]:
        subprocess.run(["cargo", "build", "--release", "--offline", "--locked",
                        "--manifest-path", str(here / "Cargo.toml"),
                        "--target-dir", str(args.output / f"{name}-target"), *features], check=True)
    workers = {name: args.output / f"{name}-target/release/hackertube-vector-bench"
               for name in ["fork", "upstream"]}
    records = []

    def worker(name, database, operation, label, **options):
        request = dict(database=str(database), operation=operation, **options)
        started = time.perf_counter()
        proc = subprocess.run([str(workers[name])], input=json.dumps(request),
                              text=True, capture_output=True, check=True)
        result = json.loads(proc.stdout)
        record = dict(label=label, worker=name, request=request, result=result,
                      process_wall_s=time.perf_counter() - started)
        (args.output / f"{label}.json").write_text(json.dumps(record))
        records.append(record)
        print(label, round(record["process_wall_s"], 3), "s", flush=True)
        return result

    template = args.output / "template.sqlite"
    backup(args.source.resolve(), template)
    worker("fork", template, "capture", "capture")
    worker("fork", template, "probe", "metadata-probe")
    with sqlite3.connect(f"file:{template}?mode=ro", uri=True) as conn:
        rows = conn.execute("SELECT rid,embedding,vid FROM benchmark_rows ORDER BY rid").fetchall()
        counts = dict(conn.execute("SELECT source,count(*) FROM videos GROUP BY source"))
        chunk_count = conn.execute("SELECT count(*) FROM chunks").fetchone()[0]
        eligible = {row[0] for row in rows}
        rng = random.Random(20261115)
        chosen = rng.sample(rows, min(64, len(rows)))
        queries = []
        for i, (rid, data, vid) in enumerate(chosen):
            values = struct.unpack("<384f", data)
            if i % 2:
                other = struct.unpack("<384f", chosen[(i + 7) % len(chosen)][1])
                values = [a * .7 + b * .3 for a, b in zip(values, other)]
                norm = math.sqrt(sum(x * x for x in values))
                values = [x / norm for x in values]
            queries.append(dict(exclude=rid, vid=vid, vector=list(values)))
        cases = [
            dict(name="similar-k6", k=6, predicate=""),
            dict(name="catalog-k80", k=80, predicate="blocked=0 AND shelf_life!='news'"),
            dict(name="agentic-k80", k=80, predicate="blocked=0 AND shelf_life!='news' AND area_agentic>=2"),
            dict(name="engineering-k80", k=80, predicate="blocked=0 AND shelf_life!='news' AND area_engineering>=2"),
            dict(name="research-k80", k=80, predicate="blocked=0 AND shelf_life!='news' AND area_research>=2"),
            dict(name="absent-source-k40", k=40, predicate="source='aietalks' AND blocked=0"),
        ]
        allowed = []
        for case in cases:
            where = " WHERE " + case["predicate"] if case["predicate"] else ""
            ids = {r[0] for r in conn.execute("SELECT rid FROM benchmark_rows" + where)}
            allowed.append(ids)
            case["eligible_before_self_exclusion"] = len(ids)
    assert len(eligible) == len(rows)
    protocol = dict(seed=20261115, rows=len(rows), sources=counts, chunks=chunk_count,
                    queries=queries, cases=cases, platform=platform.platform(),
                    warmup_rounds=1, measured_rounds=3, independent_trials=3,
                    cold_definition="fresh connection/process; OS page cache uncontrolled",
                    assessment_gate="consider ANN only if warm p95 >20ms, >=2x faster, mean tie-aware recall>=.99 and minimum>=.95, no eligible-count deficits")
    (args.output / "protocol.json").write_text(json.dumps(protocol, indent=2))
    summaries = []
    for trial in range(3):
        flat_paths = {}
        for name in ["upstream", "fork"]:
            path = args.output / f"{name}-{trial}.sqlite"
            backup(template, path)
            if name == "upstream":
                worker("fork", path, "older_writer", f"cleanup-{trial}")
            flat_paths[name] = path
        order = ["upstream", "fork"] if trial % 2 == 0 else ["fork", "upstream"]
        measured = {}
        for name in order:
            measured[name] = worker(name, flat_paths[name], "query", f"{name}-query-{trial}",
                                    queries=queries, cases=cases, rounds=4)
        baseline = {(s["case"], s["query"]): s["hits"] for s in measured["upstream"]["samples"] if s["round"] == 1}
        for name in ["upstream", "fork"]:
            for sample in measured[name]["samples"]:
                key = sample["case"], sample["query"]
                expected = baseline[key]
                assert len(sample["hits"]) == len(expected), (name, key, "count")
                # Float32 upstream accumulation differs from float64 reference by
                # ~1e-6 on this corpus; identities remain a separate exact check.
                assert all(abs(a[1]-b[1]) < 2e-6 for a, b in zip(sample["hits"], expected)), (name, key, "distance")
                assert {h[0] for h in sample["hits"]} == {h[0] for h in expected}, (name, key, "identity")
        ann_path = args.output / f"ann-{trial}.sqlite"
        backup(template, ann_path)
        worker("fork", ann_path, "build", f"ann-build-{trial}", ef=64, cache_mb=32)
        variants = [("hnsw64",64,32), ("hnsw128",128,32), ("hnsw64-cache8",64,8)]
        for name, ef, cache in variants:
            result = worker("fork", ann_path, "query", f"{name}-query-{trial}",
                            queries=queries, cases=cases, rounds=4, ann=True, ef=ef, cache_mb=cache)
            recalls, strict, deficits = [], [], []
            for sample in result["samples"]:
                ci, qi = sample["case"], sample["query"]
                expected = baseline[ci, qi]
                hits = sample["hits"]
                ids = [h[0] for h in hits]
                assert len(ids) == len(set(ids)), "duplicate ANN ids"
                assert set(ids) <= allowed[ci] - {queries[qi]["exclude"]}, "ineligible ANN hit"
                deficits.append(max(0, len(expected) - len(hits)))
                threshold = expected[-1][1] + 1e-6 if expected else 0
                recalls.append(sum(h[1] <= threshold for h in hits)/len(expected) if expected else 1.0)
                strict.append(len(set(ids) & {h[0] for h in expected})/len(expected) if expected else 1.0)
            summaries.append(dict(variant=name, trial=trial, mean_recall=statistics.mean(recalls),
                                  min_recall=min(recalls), strict_mean_recall=statistics.mean(strict),
                                  count_deficits=sum(deficits)))
        for name in order:
            worker(name, flat_paths[name], "legacy_sync", f"{name}-sync-{trial}")
            for batch in [False, True]:
                worker(name, flat_paths[name], "write", f"{name}-write-{batch}-{trial}", batch=batch)
        # Fresh process/connection for first-query costs, without claiming cold disk.
        for ci, case in enumerate(cases):
            for name, path, options in [
                ("upstream",flat_paths["upstream"],{}), ("fork",flat_paths["fork"],{}),
                ("hnsw64",ann_path,dict(ann=True,ef=64,cache_mb=32))]:
                worker("upstream" if name=="upstream" else "fork", path,"query",f"{name}-fresh-{trial}-{ci}",
                       queries=[queries[trial]],cases=[case],rounds=1,**options)
    summary = {"recall": summaries, "warm_ms": {}, "fresh_open_plus_query_ms": {},
               "write_s": {}, "sync_s": {}, "resources": {}}
    for variant in ["upstream", "fork", "hnsw64", "hnsw128", "hnsw64-cache8"]:
        selected = [r for r in records if r["label"].startswith(variant+"-query-")]
        summary["warm_ms"][variant] = {
            case["name"]: distribution([s["ms"] for r in selected for s in r["result"]["samples"]
                                       if s["round"]>0 and s["case"]==ci])
            for ci, case in enumerate(cases)}
        summary["resources"][variant] = [r["result"]["resources"] for r in selected]
    for variant in ["upstream", "fork", "hnsw64"]:
        summary["fresh_open_plus_query_ms"][variant] = {
            case["name"]: distribution([r["result"]["open_ms"] + r["result"]["samples"][0]["ms"]
                for r in records if r["label"].startswith(variant+"-fresh-") and r["label"].endswith(f"-{ci}")])
            for ci,case in enumerate(cases)}
    for name in ["upstream", "fork"]:
        summary["write_s"][name] = {str(batch): [r["result"]["write_s"] for r in records
             if r["label"].startswith(f"{name}-write-{batch}-")] for batch in [False,True]}
        summary["sync_s"][name] = [r["result"]["sync_s"] for r in records if r["label"].startswith(f"{name}-sync-")]
    summary["build_s"] = [r["result"]["build_s"] for r in records if r["label"].startswith("ann-build-")]
    summary["database_bytes"] = {p.name:p.stat().st_size for p in args.output.glob("*.sqlite")}
    summary["source_snapshot_sha256"] = hashlib.sha256(template.read_bytes()).hexdigest()
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    assert percentile([1,2,3,4], .95) == 4
    assert distribution([1,2,3])["p50"] == 2
    main()
