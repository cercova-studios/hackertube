#!/usr/bin/env python3
"""Supplement a completed run with ef=256 and empty-mark synchronization."""
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import time
from run import backup


def main(root):
    protocol = json.loads((root/'protocol.json').read_text())
    results=[]

    def worker(name, database, label, **request):
        binary=root/f'{name}-target/release/hackertube-vector-bench'
        request['database']=str(database)
        start=time.perf_counter()
        output=subprocess.run([str(binary)],input=json.dumps(request),text=True,
                              capture_output=True,check=True)
        result=json.loads(output.stdout)
        record=dict(label=label,worker=name,request=request,result=result,
                    process_wall_s=time.perf_counter()-start)
        (root/f'{label}.json').write_text(json.dumps(record))
        print(label,round(record['process_wall_s'],3),'s',flush=True)
        results.append(record)

    for trial in range(3):
        worker('fork',root/f'ann-{trial}.sqlite',f'hnsw256-query-{trial}',operation='query',
               queries=protocol['queries'],cases=protocol['cases'],rounds=4,
               ann=True,ef=256,cache_mb=32)
        for name in ['upstream','fork']:
            path=root/f'empty-{name}-{trial}.sqlite'
            backup(root/'template.sqlite',path)
            if name=='upstream':
                worker('fork',path,f'empty-cleanup-{trial}',operation='older_writer')
            with sqlite3.connect(path) as conn:
                conn.execute('UPDATE benchmark_rows SET blocked=0')
            worker(name,path,f'{name}-empty-sync-{trial}',operation='legacy_sync')
            worker(name,path,f'{name}-empty-steady-{trial}',operation='legacy_sync')
    (root/'extras.json').write_text(json.dumps(results,indent=2))


if __name__=='__main__':
    main(Path(sys.argv[1]).resolve())
