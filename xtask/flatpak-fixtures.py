#!/usr/bin/env python3
"""Genuine app bundle and disposable native-only runtime; no external download."""
import argparse, hashlib, json, os, shutil, subprocess, tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
FIXTURES=ROOT/'crates/caskade-installer/tests/fixtures'
ID='io.caskade.FixtureGame';RUNTIME='io.caskade.FixtureRuntime'
def run(args,env):
    result=subprocess.run([str(arg)for arg in args],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
    if result.returncode:raise RuntimeError(f'{args}: {result.stdout}')
    return result.stdout
def environment(job):
    return os.environ|{'FLATPAK_USER_DIR':str(job/'private-installation'),'XDG_DATA_HOME':str(job/'data'),'XDG_CONFIG_HOME':str(job/'config'),'XDG_CACHE_HOME':str(job/'cache')}
def build_runtime(output):
    # Host libraries are used only in a disposable test, never redistributed.
    with tempfile.TemporaryDirectory(prefix='caskade-flatpak-runtime-')as temp:
        job=Path(temp);env=environment(job);build=job/'runtime';(build/'usr').mkdir(parents=True);(build/'files').mkdir()
        (build/'metadata').write_text(f'[Runtime]\nname={RUNTIME}\nruntime={RUNTIME}/x86_64/1\nsdk={RUNTIME}/x86_64/1\n[Environment]\nLD_LIBRARY_PATH=/usr/lib\n')
        paths=run(['ldd',FIXTURES/'bin/fixture-game-linux-x86_64'],env);libraries=[]
        for line in paths.splitlines():
            for word in line.split():
                path=Path(word)
                if word.startswith('/')and path.is_file():libraries.append(path)
        if not libraries:raise RuntimeError('no ELF runtime libraries found')
        for library in libraries:
            target=build/'usr'/('lib64'if library.name.startswith('ld-linux')else'lib')/library.name
            target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(library,target)
            # The ELF interpreter itself must be executable inside bubblewrap.
            target.chmod(0o755 if library.name.startswith('ld-linux') else 0o644)
        run(['flatpak','build-export','--runtime','--disable-sandbox','--timestamp=2023-11-14T22:13:20Z',job/'repo',build,'1'],env)
        output.parent.mkdir(parents=True,exist_ok=True)
        run(['flatpak','build-bundle','--runtime',job/'repo',output,RUNTIME,'1'],env)
        print(f'private native runtime: {output.stat().st_size} bytes')
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--runtime-output',type=Path);args=parser.parse_args()
    if args.runtime_output:build_runtime(args.runtime_output.resolve());return
    with tempfile.TemporaryDirectory(prefix='caskade-flatpak-app-')as temp:
        job=Path(temp);env=environment(job);build=job/'app';(build/'files/bin').mkdir(parents=True);(build/'export').mkdir()
        game=build/'files/bin/fixture-game';shutil.copyfile(FIXTURES/'bin/fixture-game-linux-x86_64',game);game.chmod(0o755)
        (build/'metadata').write_text((FIXTURES/'src/flatpak/metadata').read_text())
        run(['flatpak','build-export','--disable-sandbox','--timestamp=2023-11-14T22:13:20Z',job/'repo',build,'1'],env)
        output=FIXTURES/'bin/flatpak/flatpak-bundle.flatpak';output.parent.mkdir(parents=True,exist_ok=True)
        run(['flatpak','build-bundle',job/'repo',output,ID,'1'],env)
        if output.stat().st_size>3_000_000:raise RuntimeError('fixture exceeds size budget')
        expected=FIXTURES/'expected/flatpak';expected.mkdir(parents=True,exist_ok=True)
        tree=[dict(path='bin',kind='directory',size=0,executable=False),dict(path='bin/fixture-game',kind='file',size=game.stat().st_size,sha256=hashlib.sha256(game.read_bytes()).hexdigest(),executable=True)]
        (expected/'flatpak-bundle.tree.json').write_text(json.dumps(tree,indent=2)+'\n')
        (expected/'flatpak-bundle.toml').write_text("""schema_version = 1
id = "flatpak-bundle"
title = "Caskade Fixture Game"
layout = "linux"
format = "flatpak"
platform = "linux"
input = "bin/flatpak/flatpak-bundle.flatpak"
parts = ["bin/flatpak/flatpak-bundle.flatpak"]
tree = "flatpak/flatpak-bundle.tree.json"
launch_target = "bin/fixture-game"
[host.linux]
outcome = "HandOff"
strategy = "Delegate"
runtime = "Native"
[host.windows]
outcome = "Unsupported"
strategy = "None"
runtime = "None"
[host.macos]
outcome = "Unsupported"
strategy = "None"
runtime = "None"
""")
        print(f'flatpak-bundle: {output.stat().st_size} bytes')
if __name__=='__main__':main()
