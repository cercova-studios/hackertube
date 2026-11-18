#!/usr/bin/env python3
"""Independent float64 cosine oracle for three deterministic benchmark queries."""
import json
import math
from pathlib import Path
import sqlite3
import struct
import sys


def distance(a, b):
    return 1 - sum(x*y for x,y in zip(a,b)) / math.sqrt(
        sum(x*x for x in a) * sum(y*y for y in b))


def validate(hits, oracle, allowed, k):
    expected = sorted((oracle[rid],rid) for rid in allowed)[:k]
    assert len(hits) == len(expected)
    ids = [rid for rid,_ in hits]
    assert len(ids) == len(set(ids))
    assert set(ids) <= allowed
    cutoff = expected[-1][0] + 2e-6 if expected else 0
    assert all(oracle[rid] <= cutoff for rid in ids)
    errors = [abs(score-oracle[rid]) for rid,score in hits]
    assert all(error < 2e-6 for error in errors)
    return errors


def main(root):
    protocol = json.loads((root/'protocol.json').read_text())
    errors = {name:[] for name in ['upstream','fork']}
    with sqlite3.connect(f'file:{root}/template.sqlite?mode=ro',uri=True) as conn:
        rows = conn.execute('SELECT rid,embedding FROM benchmark_rows').fetchall()
        for qi in [0,1,3]:
            query = protocol['queries'][qi]
            q = [struct.unpack('<f',struct.pack('<f',v))[0] for v in query['vector']]
            oracle = {rid:distance(q,struct.unpack('<384f',blob)) for rid,blob in rows}
            for ci,case in enumerate(protocol['cases']):
                sql = 'SELECT rid FROM benchmark_rows'
                if case['predicate']:
                    sql += ' WHERE '+case['predicate']
                allowed = {r[0] for r in conn.execute(sql)} - {query['exclude']}
                for name in errors:
                    samples=json.loads((root/f'{name}-query-0.json').read_text())['result']['samples']
                    sample=next(s for s in samples if s['round']==1 and s['case']==ci and s['query']==qi)
                    errors[name].extend(validate(sample['hits'],oracle,allowed,case['k']))
    result={name:dict(checked=len(v),max_absolute_error=max(v)) for name,v in errors.items()}
    (root/'oracle.json').write_text(json.dumps(result,indent=2))
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    assert abs(distance([3,4],[1,0])-.4)<1e-12
    try:
        validate([(9,.1)],{1:.1,9:.1},{1},1)
    except AssertionError:
        pass
    else:
        raise AssertionError('eligibility canary was not rejected')
    main(Path(sys.argv[1]).resolve())
