#!/usr/bin/env python3
"""Resume packaging after a packaging-only correction without rerunning compilation."""
import argparse, hashlib, json, os, re, shutil, subprocess, urllib.request
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target',required=True,choices=['mint22','ubuntu24.04','ubuntu26.04','debian13'])
    parser.add_argument('--run-id',required=True,type=int)
    args=parser.parse_args()
    repo=os.environ['GITHUB_REPOSITORY'];head=os.environ['GITHUB_SHA']
    def api(path):
        request=urllib.request.Request('https://api.github.com/repos/'+repo+'/'+path,
            headers={'Authorization':'Bearer '+os.environ['GH_TOKEN'],'Accept':'application/vnd.github+json'})
        with urllib.request.urlopen(request,timeout=60) as response:return json.load(response)
    run=api(f'actions/runs/{args.run_id}')
    assert run['status']=='completed','Wait for the source build to finish'
    assert run['event']=='workflow_dispatch' and run['head_branch']=='main'
    assert run['path']=='.github/workflows/linux-releases.yml'
    assert run['repository']['full_name']==repo and run['head_repository']['full_name']==repo
    source=run['head_sha'];assert re.fullmatch(r'[0-9a-f]{40}',source)
    jobs=api(f'actions/runs/{args.run_id}/jobs?per_page=100')['jobs']
    job=next(job for job in jobs if job['name']=='Build and verify '+args.target)
    for name in ['Build optimized application and converter','Preserve compiled binaries for validation retries']:
        assert next(step for step in job['steps'] if step['name']==name)['conclusion']=='success',name
    def git(*args):
        return subprocess.check_output(['git','-c','safe.directory='+str(ROOT),*args],cwd=ROOT,text=True)
    # Packaging, documentation and workflow changes can be retried. Every other
    # tracked application/build input must be byte-identical to the tested tree.
    changed=git('diff','--name-only',source,head).splitlines()
    allowed={'README.md','HISTORY.md','SOURCE_MANIFEST.sha256','.gitignore','tools/keyboard_input_smoke.py'}
    assert all(name in allowed or name.startswith(('.github/','packaging/','docs/')) for name in changed),changed
    previous=git('show',source+':.github/workflows/linux-releases.yml')
    current=(ROOT/'.github/workflows/linux-releases.yml').read_text()
    def images(workflow):return workflow.split('        include:',1)[1].split('    container:',1)[0]
    assert images(previous)==images(current),'Build environments must remain identical'
    log=ROOT/'artifacts/reused/dist'/f'tests-{args.target}.log'
    marker=ROOT/'artifacts/reused/dist/TESTS-PASSED.txt'
    text=log.read_text()
    # Older runs combined Rust/UI checks with the native GUI harness. A
    # harness-startup failure can follow fully passing Rust/UI tests. Require
    # all four successful interface summaries and the complete Rust results;
    # the current workflow always reruns the native keyboard check before
    # packaging, including when reusing binaries.
    if marker.exists():
        assert marker.read_text().strip()=='Rust and UI checks passed'
    else:
        assert next(step for step in job['steps'] if step['name']=='Run Rust and interface checks')['conclusion']=='failure'
        assert re.findall(r'Ran (\d+) tests.*?\n\nOK',text,re.S)==['18','10','8','6']
        assert text.rstrip().endswith('OK')
    assert 'test result: FAILED' not in text
    rust=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',text)))
    ui=sum(map(int,re.findall(r'Ran (\d+) tests',text)))
    assert rust>=278 and ui>=42
    output=ROOT/'dist';output.mkdir(exist_ok=True)
    shutil.copy2(log,output/log.name)
    (output/'TESTS-PASSED.txt').write_text('Rust and UI checks passed\n')
    binaries={}
    for name in ['Impress','impress-convert']:
        binary=ROOT/'target/release'/name
        assert binary.read_bytes()[:4]==b'\x7fELF'
        binaries[name]=hashlib.sha256(binary.read_bytes()).hexdigest();binary.chmod(0o755)
    provenance={'binary_source_commit':source,'packaging_source_commit':head,'source_run':run['html_url'],
        'target':args.target,'application_inputs_unchanged':True,'build_images_unchanged':True,
        'rust_tests_passed':rust,'ui_checks_passed':ui,'native_keyboard_check_rerun_required':True,'binaries':binaries}
    (output/'BINARY-PROVENANCE.json').write_text(json.dumps(provenance,indent=2)+'\n')
    with open(os.environ['GITHUB_ENV'],'a') as env:env.write('IMPRESS_BINARY_SOURCE_SHA='+source+'\n')
    print(f'Reused verified {args.target} application build: {rust} Rust tests and {ui} UI checks passed.')

if __name__=='__main__':main()
