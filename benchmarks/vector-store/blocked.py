#!/usr/bin/env python3
"""Compare captured before/after production sync functions and recommendation CLI."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
from run import backup, distribution


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['source','output','before','after','cli-before','cli-after']:
        parser.add_argument('--'+name,required=True,type=Path)
    args=parser.parse_args()
    args.output.mkdir(exist_ok=False)
    with sqlite3.connect(f'file:{args.source.resolve()}?mode=ro',uri=True) as conn:
        ids=[r[0] for r in conn.execute('SELECT vid FROM videos WHERE embed_row % 10=0 ORDER BY vid')]
    assert len(ids)==645
    cases=[('empty',[]),('mark',ids),('repeat',ids),('unblock-half',ids[::2]),('clear',[]),('empty-repeat',[])]
    records=[]
    references={}
    for trial in range(3):
        for name in (['before','after'] if trial%2==0 else ['after','before']):
            home=args.output/f'{name}-{trial}'
            home.mkdir()
            database=home/'catalog.sqlite'
            backup(args.source.resolve(),database)
            for label,vids in cases:
                request=dict(database=str(database),vids=vids)
                result=subprocess.run([str(getattr(args,name).resolve())],input=json.dumps(request),
                                      text=True,capture_output=True,check=True)
                doc=json.loads(result.stdout)
                assert not result.stderr
                assert doc['blocked_counts']==[len(vids),len(vids),0],doc
                if name=='after' and label in ['empty','repeat','empty-repeat']:
                    assert doc['changes']==0,doc
                record=dict(variant=name,trial=trial,case=label,**doc)
                records.append(record)
                print(name,trial,label,round(doc['ms'],3),'ms',flush=True)
            marks={vid:dict(status='seen',at='2026-11-15T00:00:00Z',until=None) for vid in ids}
            (home/'state.json').write_text(json.dumps(dict(version=1,items=marks)))
            for repeat in range(2):
                started=time.perf_counter()
                result=subprocess.run([str(getattr(args,'cli_'+name).resolve()),'recommend','--area','ai-engineering','--json'],
                    env=dict(os.environ,HOME=str(home),HACKERTUBE_HOME=str(home),HACKERTUBE_EMBED='off',
                             HACKERTUBE_CAPTIONS='off',HACKERTUBE_TODAY='2026-11-15'),
                    text=True,capture_output=True,check=True)
                elapsed=(time.perf_counter()-started)*1000
                assert not result.stderr
                doc=json.loads(result.stdout)
                key=trial,repeat
                if key in references:assert references[key]==doc,'recommend output changed'
                references[key]=doc
                records.append(dict(variant=name,trial=trial,case=f'cli-{repeat}',ms=elapsed,document=doc))
                print(name,trial,f'cli-{repeat}',round(elapsed,3),'ms',flush=True)
    summary={name:{case:distribution([r['ms'] for r in records if r['variant']==name and r['case']==case])
                   for case in [c[0] for c in cases]+['cli-0','cli-1']} for name in ['before','after']}
    (args.output/'records.json').write_text(json.dumps(records,indent=2))
    (args.output/'summary.json').write_text(json.dumps(summary,indent=2))
    print(json.dumps(summary,indent=2))


if __name__=='__main__':
    main()
