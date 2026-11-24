#!/usr/bin/env python3
"""Publish only validated artifacts from the same GitHub Actions source commit."""
import argparse, json, os, subprocess, tempfile
from pathlib import Path

GROUPS = {
    'mint22': ('Linux Mint 22.x', ['mint22']),
    'debian13': ('Debian 13', ['debian13']),
    'ubuntu': ('Ubuntu 24.04 / 26.04 LTS', ['ubuntu24.04','ubuntu26.04']),
}

def gh(*args): return subprocess.check_output(['gh',*args],text=True).strip()
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--version',required=True);parser.add_argument('--artifacts',type=Path,required=True);args=parser.parse_args()
    repo=os.environ['GITHUB_REPOSITORY'];commit=os.environ['GITHUB_SHA']
    for suffix,(label,targets) in GROUPS.items():
        assets=[];notes=[]
        for target in targets:
            folder=args.artifacts/target
            info=json.loads((folder/f'BUILD-INFO-{target}.json').read_text())
            assert info['source_commit']==commit and info['application_version']==args.version
            assert (folder/'validation/RESULT.txt').read_text().startswith('Passed:')
            assert (folder/'TESTS-PASSED.txt').read_text().strip()=='Rust and UI checks passed'
            assets.extend(folder.glob('*.deb'))
            notes.append(f"- **{info['target']}**, amd64 — built in {info.get('build_environment',info['build_os'].get('PRETTY_NAME',info['target']))}; dependencies generated from that environment.")
            if info.get('validation_provenance'):
                original=info['validation_provenance']
                notes.append(f"  Binaries and passing tests came from [this build]({original['source_run']}) at `{info['binary_source_commit']}`. Application inputs and pinned build images were verified unchanged for this packaging revision; installation and startup checks were run again.")
        assert len([path for path in assets if path.suffix=='.deb'])==len(targets)
        with tempfile.TemporaryDirectory(prefix='impress-release-') as tmp:
            tmp=Path(tmp)
            body=tmp/'notes.md';body.write_text(f'''Distribution-specific packages of Impress {args.version} for **{label}**.

{chr(10).join(notes)}

Download the `.deb` matching your distribution, then install it with `sudo apt install ./impress_*.deb` from the download folder. These packages include both the desktop application and `impress-convert`.

Each package was installed with its target distribution's package manager, checked for unresolved shared libraries, exercised through a DOCX conversion round trip, and launched under Xvfb. Rust and UI resource tests passed before publication. Build and validation logs remain available in GitHub Actions. These are container/X11 checks, not a claim of testing every desktop configuration.

See the [release notes](https://github.com/{repo}/releases/tag/v{args.version}) for changes in this version.

Source commit: `{commit}`. See the repository [README](https://github.com/{repo}#readme) for an introduction and [HISTORY.md](https://github.com/{repo}/blob/{commit}/HISTORY.md) for changes.
''')
            tag=f'v{args.version}-{suffix}'
            existing=subprocess.run(['gh','release','view',tag,'--repo',repo],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0
            if existing:
                remote=json.loads(gh('api',f'repos/{repo}/git/ref/tags/{tag}'))
                assert remote['object']['type']=='commit' and remote['object']['sha']==commit,'Refusing to replace assets for a different source commit'
                gh('release','upload',tag,*map(str,assets),'--repo',repo,'--clobber')
                gh('release','edit',tag,'--repo',repo,'--title',f'Impress {args.version} — {label} (amd64)','--notes-file',str(body))
            else:
                gh('release','create',tag,*map(str,assets),'--repo',repo,'--target',commit,'--title',f'Impress {args.version} — {label} (amd64)','--notes-file',str(body),'--latest=false')
            print(gh('release','view',tag,'--repo',repo,'--json','url','--jq','.url'),flush=True)

if __name__=='__main__':main()
