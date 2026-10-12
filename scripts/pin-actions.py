#!/usr/bin/env python3
"""Pin every `uses: owner/repo@ref` in .github/workflows to a full commit SHA (supply chain).

Resolves the newest tag within the same major (or the branch head for branch refs) with
`git ls-remote`, rewrites to `owner/repo@<sha> # <tag>`. Idempotent. `make github-action-bump`
re-runs it. No tokens needed.
"""
import glob, re, subprocess, sys

def ls(repo, pattern):
    out = subprocess.run(["git", "ls-remote", f"https://github.com/{repo}", pattern], capture_output=True, text=True, check=True).stdout
    refs = {}
    for line in out.splitlines():
        sha, ref = line.split("\t")
        name = ref.replace("refs/tags/", "").replace("refs/heads/", "")
        deref = name.endswith("^{}")
        name = name.removesuffix("^{}")
        if deref or name not in refs:
            refs[name] = sha          # dereferenced (annotated) commit wins
    return refs

def key(tag):
    return [int(x) for x in re.findall(r"\d+", tag)]

def resolve(repo, ref):
    if re.fullmatch(r"[0-9a-f]{40}", ref):
        return ref, None
    if re.fullmatch(r"v\d+", ref):                       # major tag: newest v<major>.*
        refs = ls(repo, f"refs/tags/{ref}.*")
        full = [t for t in refs if re.fullmatch(rf"{ref}\.\d+\.\d+", t)]
        if full:
            t = max(full, key=key)
            return refs[t], t
        refs = ls(repo, f"refs/tags/{ref}")
        return refs[ref], ref
    refs = ls(repo, f"refs/heads/{ref}") or ls(repo, f"refs/tags/{ref}")
    return refs[ref], ref

changed = 0
for path in glob.glob(".github/workflows/*.yml"):
    s = open(path).read()
    def sub(m):
        global changed
        indent, repo, ref = m.group(1), m.group(2), m.group(3)
        sha, tag = resolve(repo, ref)
        if tag is None:
            return m.group(0)
        changed += 1
        return f"{indent}uses: {repo}@{sha} # {tag}"
    s2 = re.sub(r"^(\s*-?\s*)uses: ([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)@([A-Za-z0-9._-]+)\s*(?:#.*)?$", sub, s, flags=re.M)
    open(path, "w").write(s2)
print(f"pinned {changed} references")
