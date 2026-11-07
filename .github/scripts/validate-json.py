"""Reject JSON that is malformed or that repeats a key.

`python3 -m json.tool` accepts duplicate keys, so a union of two conflicting
sides could publish a locale file with the same key twice (last one silently
wins). Such a file is treated as unresolved and falls back to the upstream
version instead.
"""

import json
import sys


def reject_duplicate_keys(pairs):
    seen = set()
    for key, _ in pairs:
        if key in seen:
            raise ValueError(f'duplicate key: {key}')
        seen.add(key)
    return dict(pairs)


with open(sys.argv[1], encoding='utf-8') as handle:
    json.load(handle, object_pairs_hook=reject_duplicate_keys)
