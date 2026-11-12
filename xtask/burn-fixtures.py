#!/usr/bin/env python3
"""Build an original WiX Burn bundle; keep compiler/runtime binaries ignored."""
import argparse, hashlib, json, os, pathlib, shutil, subprocess, urllib.request, zipfile
ROOT=pathlib.Path(__file__).resolve().parents[1]
FIX=ROOT/'crates/caskade-installer/tests/fixtures'
SRC=FIX/'src/burn'
CACHE=ROOT/'target/fixture-build/burn-tools'
def win(path):return 'Z:'+str(path.resolve()).replace('/','\\')
def download(pin,name):
    path=CACHE/name
    if not path.exists():urllib.request.urlretrieve(pin['url'],path)
    if hashlib.sha256(path.read_bytes()).hexdigest()!=pin['sha256']:raise RuntimeError('builder checksum mismatch: '+name)
    return path

def main():
    args=argparse.ArgumentParser();args.add_argument('--artifact',type=pathlib.Path);opts=args.parse_args()
    pins=json.loads((SRC/'builders.json').read_text());CACHE.mkdir(parents=True,exist_ok=True)
    wix=download(pins['wix'],'wix314-binaries.zip');mono=download(pins['wine_mono'],'wine-mono-9.4.0-x86.msi')
    tools=CACHE/'wix'
    if not (tools/'candle.exe').exists():
        with zipfile.ZipFile(wix) as archive:
            for item in archive.infolist():
                name=pathlib.PurePosixPath(item.filename.replace('\\','/'))
                if name.is_absolute() or '..' in name.parts:raise RuntimeError('unsafe builder ZIP')
            archive.extractall(tools)
    env=os.environ|{'WINEPREFIX':str(CACHE/'prefix'),'WINEDEBUG':'-all','WINEDLLOVERRIDES':'winemenubuilder.exe=d'}
    # A desktop of its own (#122): xvfb-run alone leaves Wine on the developer's session bus.
    def wine(argv,cwd=None):subprocess.run([str(ROOT/'scripts/private-desktop.sh'),'--','wine',*map(str,argv)],cwd=cwd,env=env,check=True,timeout=180)
    if not (CACHE/'prefix/drive_c/windows/mono').exists():wine(['msiexec','/i',win(mono),'/qn','/norestart'])
    build=CACHE/'build';build.mkdir(exist_ok=True)
    wine([win(tools/'candle.exe'),'-nologo','-ext',win(tools/'WixBalExtension.dll'),'-dPayload='+win(FIX/pins['payload']),'-out',win(build/'bundle.wixobj'),win(SRC/'bundle.wxs')],build)
    wine([win(tools/'light.exe'),'-nologo','-ext',win(tools/'WixBalExtension.dll'),'-out',win(build/'burn-bundle.exe'),win(build/'bundle.wixobj')],build)
    binary=build/'burn-bundle.exe'
    if binary.stat().st_size>3_000_000:raise RuntimeError('fixture exceeds 3MB')
    destination=(opts.artifact or FIX)/'bin/burn/burn-bundle.exe';destination.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(binary,destination)
    expected=(opts.artifact or FIX)/'expected/burn';expected.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(FIX/'expected/windows/msi-embedded.tree.json',expected/'burn-bundle.tree.json')
    (expected/'burn-bundle.toml').write_text("""schema_version = 1
id = "burn-bundle"
title = "Fixture Game"
layout = "unity"
format = "burn"
platform = "windows"
input = "bin/burn/burn-bundle.exe"
parts = ["bin/burn/burn-bundle.exe"]
tree = "burn/burn-bundle.tree.json"
launch_target = "FixtureGame.exe"

[windows]
builder = "WiX Burn"
version = "3.14.1"
features = ["embedded-msi", "InstallFolder-variable", "per-user"]
native_generated = []

[host.windows]
outcome = "Installed"
strategy = "RunSilent"
runtime = "Native"

[host.linux]
outcome = "Installed"
strategy = "Extract"
runtime = "ProtonPrefix"

[host.macos]
outcome = "Installed"
strategy = "Extract"
runtime = "WinePrefix"
""")
    print('genuine Burn fixture:',binary.stat().st_size,'bytes')
if __name__=='__main__':main()
