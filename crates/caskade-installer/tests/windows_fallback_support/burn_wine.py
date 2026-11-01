#!/usr/bin/env python3
"""Native-profile Wine pre-evidence, not a Windows CI acceptance claim.
Run under flock /tmp/caskade-heavy.lock; never automate wizard controls.
"""
import hashlib,json,os,pathlib,shutil,subprocess,tempfile,time
ROOT=pathlib.Path(__file__).resolve().parents[4]
FIX=ROOT/'crates/caskade-installer/tests/fixtures'
WORK=pathlib.Path(tempfile.mkdtemp(prefix='caskade-burn-wine-'))
ENV=os.environ|{'WINEPREFIX':str(WORK/'prefix'),'WINEDEBUG':'-all','WINEDLLOVERRIDES':'winemenubuilder.exe=d'}
def win(path):return 'Z:'+str(path.resolve()).replace('/','\\')
def run(args):
    with tempfile.TemporaryFile() as output:
        result=subprocess.run(args,env=ENV,cwd=WORK,stdout=output,stderr=subprocess.STDOUT,timeout=120)
        output.seek(0); details=output.read().decode(errors='replace')
    if result.returncode:raise RuntimeError((args,result.returncode,details))
    return result
try:
    run(['wineboot','-u'])
    destination=WORK/'Café Space Burn';destination.mkdir()
    bundle=FIX/'bin/burn/burn-bundle.exe'
    run(['wine',win(bundle),'/quiet','/norestart','/log',win(WORK/'burn-install.log'),'InstallFolder='+win(destination)])
    until=time.monotonic()+20
    while not (destination/'FixtureGame.exe').exists() and time.monotonic()<until:time.sleep(0.05)
    entries=[]
    for path in sorted(destination.rglob('*')):
        data=path.read_bytes() if path.is_file() else b''
        entry={'path':path.relative_to(destination).as_posix(),'kind':'file' if path.is_file() else 'directory','size':len(data),'executable':False}
        if path.is_file():entry['sha256']=hashlib.sha256(data).hexdigest()
        entries.append(entry)
    assert entries==json.loads((FIX/'expected/burn/burn-bundle.tree.json').read_text()),(entries,(WORK/'burn-install.log').read_text(errors='replace')[-12000:] if (WORK/'burn-install.log').exists() else 'no Burn log',list(WORK.rglob('FixtureGame.exe')))
    token=WORK/'burn-token.json'
    run(['wine',win(destination/'FixtureGame.exe'),'--token',win(token)])
    assert token.is_file()
    run(['wine',win(bundle),'/uninstall','/quiet','/norestart','/log',win(WORK/'burn-uninstall.log')])
    until=time.monotonic()+20
    while (destination/'FixtureGame.exe').exists() and time.monotonic()<until:time.sleep(0.05)
    assert not (destination/'FixtureGame.exe').exists(),(WORK/'burn-uninstall.log').read_text(errors='replace')[-12000:]
    assert all(not (destination/entry['path']).exists() for entry in entries if entry['kind']=='file'), 'uninstall left payload files'
    output=ROOT/'target/native-windows/burn-wine-pre-evidence.json';output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps({'profile':'Wine pre-evidence','native_windows':False,'fixture':'burn-bundle','builder':'WiX 3.14.1','sha256':hashlib.sha256(bundle.read_bytes()).hexdigest(),'quiet_install':True,'exact_tree':True,'actual_token_launch':True,'quiet_uninstall':True,'install_folder_overridable':True,'install_folder_persisted':True,'argv':['/quiet','/norestart','InstallFolder=<private Café Space Burn>']},indent=2)+'\n')
    print('Burn Wine pre-evidence: quiet install / exact tree / token launch / quiet uninstall PASS')
finally:
    subprocess.run(['wineserver','-k'],env=ENV,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    shutil.rmtree(WORK)
