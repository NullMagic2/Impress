#!/usr/bin/env python3
"""Package binaries built inside the named distribution; never loosen ABI deps."""
import argparse, gzip, hashlib, json, os, re, shutil, subprocess, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGETS = {
    'mint22': ('Linux Mint 22.x', 'linuxmint', '22', 'noble'),
    'ubuntu24.04': ('Ubuntu 24.04 LTS', 'ubuntu', '24.04', 'noble'),
    'ubuntu26.04': ('Ubuntu 26.04 LTS', 'ubuntu', '26.04', 'resolute'),
    'debian13': ('Debian 13', 'debian', '13', 'trixie'),
}

def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def run(args, **kwargs): return subprocess.check_output(args, text=True, **kwargs).strip()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', choices=TARGETS, required=True)
    parser.add_argument('--version', default='0.8.6-59')
    parser.add_argument('--binaries', type=Path, default=ROOT/'target/release')
    parser.add_argument('--output', type=Path, default=ROOT/'dist')
    args = parser.parse_args()
    if not re.fullmatch(r'\d+\.\d+\.\d+-\d+', args.version): parser.error('Use a numeric version and package revision')
    label, os_id, os_version, suite = TARGETS[args.target]
    release = dict(line.strip().split('=', 1) for line in Path('/etc/os-release').read_text().splitlines() if '=' in line)
    release = {key: value.strip('"') for key, value in release.items()}
    assert release.get('ID') == os_id and (release.get('VERSION_ID') == os_version or (os_id=='linuxmint' and release.get('VERSION_ID','').startswith('22.'))), release
    assert run(['dpkg','--print-architecture']) == 'amd64'
    args.output.mkdir(parents=True, exist_ok=True)
    version = args.version+'+'+args.target
    with tempfile.TemporaryDirectory(prefix='impress-deb-') as tmp:
        base=Path(tmp); stage=base/'stage'
        def install(source, relative, mode=0o644):
            target=stage/relative;target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source,target);target.chmod(mode)
        def write(relative, content, mode=0o644):
            target=stage/relative;target.parent.mkdir(parents=True,exist_ok=True);target.write_text(content);target.chmod(mode)
        for name in ['Impress','impress-convert']:
            binary=args.binaries/name;assert binary.is_file(),binary
            install(binary,'usr/lib/impress/'+name,0o755)
        for command,binary in [('impress','Impress'),('impress-convert','impress-convert')]:
            write('usr/bin/'+command,'#!/bin/sh\nexec /usr/lib/impress/'+binary+' "$@"\n',0o755)
        desktop=(ROOT/'packaging/linux/Impress.desktop.in').read_text().replace('@EXEC@','/usr/bin/impress')
        desktop=desktop.replace('MimeType=', 'MimeType=application/x-impress;application/msword;application/pdf;text/x-typst;')
        write('usr/share/applications/Impress.desktop',desktop)
        install(ROOT/'design/icons/color/logo.svg','usr/share/icons/hicolor/scalable/apps/Impress.svg')
        write('usr/share/mime/packages/impress.xml','''<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
 <mime-type type="application/x-impress"><comment>Impress document</comment><glob pattern="*.impress"/><sub-class-of type="application/zip"/></mime-type>
</mime-info>
''')
        for source in [ROOT/'LICENSE',ROOT/'LICENSE.md',ROOT/'README.md',ROOT/'HISTORY.md',*sorted((ROOT/'docs').glob('*.md'))]:
            if source.is_file(): install(source,'usr/share/doc/impress/'+source.name)
        # Preserve the license notices for embedded artwork and vendored code.
        for folder in [ROOT/'vendor',ROOT/'crates/gui/assets',ROOT/'crates/typography/tests/fixtures']:
            for source in folder.rglob('*'):
                if source.is_file() and any(word in source.name.upper() for word in ['LICENSE','COPYRIGHT','COPYING','YARU_ICONS']):
                    install(source,'usr/share/doc/impress/licenses/'+str(source.relative_to(ROOT)))
        (base/'debian').mkdir()
        (base/'debian/control').write_text('Source: impress\nSection: editors\nPriority: optional\nMaintainer: NMagic <28734029+NullMagic2@users.noreply.github.com>\n\nPackage: impress\nArchitecture: any\nDescription: desktop word processor\n')
        shlibs=run(['dpkg-shlibdeps','-O','-e'+str(stage/'usr/lib/impress/Impress'),'-e'+str(stage/'usr/lib/impress/impress-convert')],cwd=base)
        dependency=next(line.split('=',1)[1] for line in shlibs.splitlines() if line.startswith('shlibs:Depends='))
        dependency='xkb-data, libenchant-2-2, fonts-noto-core, '+dependency
        size=sum(p.stat().st_size for p in (stage/'usr').rglob('*') if p.is_file())//1024
        write('DEBIAN/control',f'''Package: impress
Version: {version}
Section: editors
Priority: optional
Architecture: amd64
Maintainer: NMagic <28734029+NullMagic2@users.noreply.github.com>
Homepage: https://github.com/NullMagic2/Impress
Installed-Size: {size}
Depends: {dependency}
Recommends: hicolor-icon-theme, fonts-dejavu-core, fonts-liberation
Suggests: tesseract-ocr, hunspell-en-us, hunspell-el
Description: word processor with Typst equations and local spellchecking
 Paginated document editing with styles, tables, pictures and equations.
 Supports native Impress, DOCX, DOC, RTF, Typst, PDF output and PDF import.
 Includes the impress-convert command-line converter.
 .
 Built for {label}, amd64, using its own system libraries.
''')
        for hook in ['postinst','postrm']:
            write('DEBIAN/'+hook,'''#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database -q /usr/share/applications || true; fi
if command -v update-mime-database >/dev/null 2>&1; then update-mime-database /usr/share/mime || true; fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true; fi
exit 0
''',0o755)
        write('usr/share/doc/impress/copyright',(ROOT/'LICENSE').read_text() if (ROOT/'LICENSE').exists() else (ROOT/'LICENSE.md').read_text())
        source_commit=os.environ.get('GITHUB_SHA') or run(['git','rev-parse','HEAD'],cwd=ROOT)
        info={'application_version':args.version,'package_version':version,'target':label,'architecture':'amd64','build_os':release,'source_commit':source_commit,'binary_source_commit':os.environ.get('IMPRESS_BINARY_SOURCE_SHA',source_commit),'rust':run(['rustc','--version']),'dependencies':dependency,'binaries':{name:digest(args.binaries/name) for name in ['Impress','impress-convert']}}
        provenance=args.output/'BINARY-PROVENANCE.json'
        if provenance.exists(): info['validation_provenance']=json.loads(provenance.read_text())
        info_path=args.output/f'BUILD-INFO-{args.target}.json';info_path.write_text(json.dumps(info,indent=2)+'\n');install(info_path,'usr/share/doc/impress/BUILD-INFO.json')
        subprocess.run(['desktop-file-validate',str(stage/'usr/share/applications/Impress.desktop')],check=True)
        package=args.output/f'impress_{args.version}_{args.target}_amd64.deb'
        subprocess.run(['dpkg-deb','--root-owner-group','--build',str(stage),str(package)],check=True)
        subprocess.run(['dpkg-deb','--info',str(package)],check=True)
        assert run(['dpkg-deb','-f',str(package),'Version'])==version
        # Validate the packaged bytes, including both public launchers.
        extracted=base/'extracted';subprocess.run(['dpkg-deb','-x',str(package),str(extracted)],check=True)
        for name in ['Impress','impress-convert']: assert digest(extracted/'usr/lib/impress'/name)==info['binaries'][name]
        for name in ['impress','impress-convert']: assert (extracted/'usr/bin'/name).read_text().startswith('#!/bin/sh\nexec /usr/lib/impress/')
        print(package)

if __name__=='__main__': main()
