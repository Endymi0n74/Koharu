import re, glob, collections, itertools

runs = {}
for f in sorted(glob.glob("target/flip-study/run-*.llmdbg.log")):
    n = re.search(r"run-(\d+)", f).group(1)
    calls, cur = [], None
    for line in open(f, encoding="utf-8", errors="replace"):
        m = re.match(r"LLMDBG prompt ([0-9a-f]{16})", line)
        if m: cur = {"prompt": m.group(1), "steps": []}; calls.append(cur); continue
        m = re.match(r"LLMDBG shape prompt_tokens=(\d+)", line)
        if m and cur: cur["ptoks"] = int(m.group(1)); continue
        m = re.match(r"LLMDBG history ([0-9a-f]{16})", line)
        if m and cur: cur["hist"] = m.group(1); continue
        m = re.match(r"LLMDBG first_token ([0-9a-f]{16})", line)
        if m and cur: cur["first"] = m.group(1); continue
        m = re.match(r"LLMDBG step=(\d+) tok=(-?\d+)", line)
        if m and cur: cur["steps"].append(int(m.group(2))); continue
        m = re.match(r"LLMDBG text ([0-9a-f]{16})", line)
        if m and cur: cur["text"] = m.group(1); continue
        m = re.match(r"LLMDBG tokens generated=(\d+)", line)
        if m and cur: cur["gen"] = int(m.group(1))
    runs[n] = calls

# group by identical prompt (eliminates chapter-context contamination)
groups = collections.defaultdict(list)
for n, calls in runs.items():
    for c in calls:
        groups[c["prompt"]].append((n, c))

pairs = flips = text_flips = first_flips = 0
tokens_generated = tokens_compared = 0
flip_steps = []
print(f"{'prompt':10} {'runs':18} {'flips':>5} {'texts'}")
for p, members in sorted(groups.items(), key=lambda kv: -len(kv[1])):
    if len(members) < 2:
        continue
    ref_n, ref = members[0]
    g_flips = 0
    g_texts = {ref["text"]}
    for n, c in members[1:]:
        pairs += 1
        tokens_generated += c["gen"]
        g_texts.add(c["text"])
        if c["text"] != ref["text"]:
            text_flips += 1
        if c["first"] != ref["first"]:
            first_flips += 1
        k = 0
        for a, b in zip(ref["steps"], c["steps"]):
            k += 1
            if a != b:
                flips += 1
                g_flips += 1
                flip_steps.append((p, f"{ref_n}->{n}", k))
                break
        tokens_compared += k
    label = ",".join(n for n, _ in members)
    print(f"{p[:8]:10} [{label:18}] {g_flips:5} {len(g_texts)} distinct")
    for (pp, pair, k) in flip_steps:
        if pp == p:
            print(f"           flip at step {k} ({pair})")

print(f"\ncomparable pairs (same prompt): {pairs}")
print(f"token-stream flips: {flips}")
print(f"text-hash mismatches: {text_flips}")
print(f"first-token mismatches: {first_flips}")
print(f"tokens generated in pairs: {tokens_generated}")
if tokens_generated:
    print(f"flip rate: {1000.0*flips/tokens_generated:.3f} flips / 1000 tokens")
    print(f"flip rate: {100.0*flips/pairs:.1f} % of same-prompt calls")
if flip_steps:
    ks = [k for (_, _, k) in flip_steps]
    print(f"divergence step: min={min(ks)} median={sorted(ks)[len(ks)//2]} max={max(ks)}")
