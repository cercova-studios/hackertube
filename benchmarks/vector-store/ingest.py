#!/usr/bin/env python3
"""Time matched full CLI refresh/no-op ingests on disposable catalog backups."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
from run import backup


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['run','source','data','fork-bin','upstream-bin']:
        parser.add_argument('--'+name,required=True,type=Path)
    args=parser.parse_args()
    out=args.run/'full-ingest'
    out.mkdir(exist_ok=False)
    worker=args.run/'fork-target/release/hackertube-vector-bench'
    all_results=[]
    for trial in range(3):
        order=['upstream','fork'] if trial%2==0 else ['fork','upstream']
        for name in order:
            home=out/f'{name}-{trial}'
            home.mkdir()
            database=home/'catalog.sqlite'
            backup(args.source.resolve(),database)
            if name=='upstream':
                request=dict(database=str(database),operation='older_writer')
                subprocess.run([str(worker)],input=json.dumps(request),text=True,
                               capture_output=True,check=True)
            binary=getattr(args,name+'_bin').resolve()
            for refresh in [True,False]:
                label=f'{name}-{trial}-'+('refresh' if refresh else 'noop')
                command=[str(binary),'ingest','--source','hackertube','--path',str(args.data.resolve()),'--json']
                if refresh:
                    command.append('--refresh')
                env=dict(os.environ,HOME=str(home),HACKERTUBE_HOME=str(home),
                         HACKERTUBE_EMBED='off',HACKERTUBE_CAPTIONS='off')
                with (out/f'{label}.stdout').open('w') as stdout, (out/f'{label}.stderr').open('w') as stderr:
                    started=time.perf_counter()
                    proc=subprocess.Popen(command,env=env,stdout=stdout,stderr=stderr)
                    _,status,usage=os.wait4(proc.pid,0)
                    proc.returncode=os.waitstatus_to_exitcode(status)
                    elapsed=time.perf_counter()-started
                assert proc.returncode==0,(label,(out/f'{label}.stdout').read_text())
                assert not (out/f'{label}.stderr').read_text()
                document=json.loads((out/f'{label}.stdout').read_text())
                source=document['sources'][0]
                assert source['videos']==6454 and source['embedded']==6454,source
                assert source['changed'] is refresh,source
                assert not (home/'state.json').exists()
                assert not (home/'models').exists()
                with sqlite3.connect(f'file:{database}?mode=ro',uri=True) as conn:
                    assert conn.execute('SELECT count(*) FROM videos').fetchone()[0]==6454
                record=dict(label=label,command=command,wall_s=elapsed,
                            cpu_s=usage.ru_utime+usage.ru_stime,
                            max_rss_bytes=usage.ru_maxrss if os.uname().sysname=='Darwin' else usage.ru_maxrss*1024,
                            source=source)
                (out/f'{label}.json').write_text(json.dumps(record,indent=2))
                all_results.append(record)
                print(label,round(elapsed,3),'s',flush=True)
    (out/'summary.json').write_text(json.dumps(all_results,indent=2))


if __name__=='__main__':
    main()
