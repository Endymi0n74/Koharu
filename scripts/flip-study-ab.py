#!/usr/bin/env python3
"""A/B flip-rate comparison for KOHARU_LLM_DEBUG runs.

Groups LLMDBG token streams (`LLMDBG step=N tok=M`) by identical prompt hash
(`LLMDBG prompt <hex>`), compares every pair of streams across runs, and
reports the flip rate per 1,000 generated tokens (first divergence step).

Usage: flip-study-ab.py <dir1> [<dir2> ...]
Each directory is one experimental condition and must contain one *.log file
per run. Streams are only compared between runs (never within one run).
"""

import collections
import glob
import itertools
import os
import re
import sys

STEP = re.compile(r"LLMDBG step=(\d+) tok=(\d+)")
PROMPT = re.compile(r"LLMDBG prompt ([0-9a-f]+)")


def collect(directory):
    """One run per *.log file: {prompt: [stream, ...]} with streams kept separate."""
    runs = []
    for path in sorted(glob.glob(os.path.join(directory, "*.log"))):
        calls = {}
        current = None
        stream = []
        with open(path, encoding="utf-8", errors="replace") as handle:
            for line in handle:
                match = PROMPT.search(line)
                if match:
                    if current is not None and stream:
                        calls.setdefault(current, []).append(stream)
                    current = match.group(1)
                    stream = []
                    continue
                match = STEP.search(line)
                if match and current is not None:
                    stream.append(int(match.group(2)))
        if current is not None and stream:
            calls.setdefault(current, []).append(stream)
        runs.append(calls)
    return runs


def first_divergence(stream_a, stream_b):
    for index, (tok_a, tok_b) in enumerate(zip(stream_a, stream_b)):
        if tok_a != tok_b:
            return index
    return None


def main():
    directories = sys.argv[1:]
    if len(directories) < 2:
        sys.exit(f"usage: {sys.argv[0]} <dir1> <dir2> ...")

    conditions = {os.path.basename(os.path.normpath(d)): collect(d) for d in directories}

    for name, runs in conditions.items():
        pairs = diverged = tokens = 0
        steps = collections.Counter()
        per_prompt = collections.defaultdict(lambda: [0, 0])
        for run_a, run_b in itertools.combinations(runs, 2):
            for prompt, streams in run_a.items():
                for stream_a in streams:
                    for stream_b in run_b.get(prompt, []):
                        pairs += 1
                        tokens += len(stream_a)
                        per_prompt[prompt][0] += 1
                        step = first_divergence(stream_a, stream_b)
                        if step is not None:
                            diverged += 1
                            steps[step] += 1
                            per_prompt[prompt][1] += 1
        rate = 1000.0 * diverged / tokens if tokens else 0.0
        print(f"[{name}] runs={len(runs)} pairs={pairs} diverged={diverged} "
              f"({100.0 * diverged / pairs if pairs else 0:.1f}%) "
              f"tokens={tokens} flip_rate={rate:.2f}/1000")
        for prompt, (prompt_pairs, prompt_diverged) in sorted(per_prompt.items()):
            print(f"    prompt {prompt[:16]}… pairs={prompt_pairs} diverged={prompt_diverged}")
        if steps:
            top = ", ".join(str(step) for step, _ in steps.most_common(8))
            print(f"    divergence steps (top): {top}")

    (name_a, runs_a), (name_b, runs_b) = list(conditions.items())[:2]
    pairs = diverged = tokens = 0
    steps = collections.Counter()
    for run_a in runs_a:
        for run_b in runs_b:
            for prompt, streams in run_a.items():
                for stream_a in streams:
                    for stream_b in run_b.get(prompt, []):
                        pairs += 1
                        tokens += len(stream_a)
                        step = first_divergence(stream_a, stream_b)
                        if step is not None:
                            diverged += 1
                            steps[step] += 1
    rate = 1000.0 * diverged / tokens if tokens else 0.0
    print(f"[A/B {name_a} vs {name_b}] cross pairs={pairs} diverged={diverged} "
          f"({100.0 * diverged / pairs if pairs else 0:.1f}%) tokens={tokens} "
          f"flip_rate={rate:.2f}/1000")
    if steps:
        top = ", ".join(str(step) for step, _ in steps.most_common(8))
        print(f"    divergence steps (top): {top}")


if __name__ == "__main__":
    main()
