#!/usr/bin/env python3
"""Create fresh prefixes, import original keys, export only their sections.
No existing prefix, launcher data, account or registry is ever read.
"""
import argparse, os, pathlib, re, subprocess, tempfile

def raw_string(value):
    return ','.join(f'{b:02x}' for b in (value+'\0').encode('utf-16-le'))

def import_text():
    text='Windows Registry Editor Version 5.00\n'
    for hive in ['HKEY_LOCAL_MACHINE','HKEY_CURRENT_USER']:
        for suffix in ['Software\\CaskadeDiscoveryFixture','Software\\Wow6432Node\\CaskadeDiscoveryFixture']:
            text+=f'\n[{hive}\\{suffix}]\n'
            for name,kind,value in [
                ('InstallPath',1,'C:\\Users\\fixture\\Games\\游戏 🎮'),
                ('Expand',2,'%USERPROFILE%\\Games'),
                ('Multi',7,'one\0two\0'),
            ]:
                text+=f'"{name}"=hex({kind:x}):{raw_string(value)}\n'
            text+='"Count"=dword:12345678\n"Big"=hex(b):08,07,06,05,04,03,02,01\n"Blob"=hex:00,01,ff\n'
            text+=f'\n[{hive}\\{suffix}\\Empty]\n'
    return text

def extract(path):
    text=path.read_text()
    arch=re.search(r'^#arch=.*$',text,re.M)
    output=['WINE REGISTRY Version 2',';; Only original CaskadeDiscoveryFixture keys from a fresh generated prefix.',arch[0] if arch else '#arch=win64','']
    keep=False
    for line in text.splitlines():
        if line.startswith('['):
            keep='CaskadeDiscoveryFixture' in line
            if keep: line=re.sub(r'\] \d+$','] 1700000000',line)
        if keep:
            if line.startswith('#time='):line='#time=1da1747c66d0000'
            output.append(line)
    result='\n'.join(output).rstrip()+'\n'
    if '"InstallPath"=' not in result:raise RuntimeError('authored import absent')
    return result

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=pathlib.Path,required=True)
    parser.add_argument('--wine',default='wine')
    parser.add_argument('--umu',type=pathlib.Path,required=True)
    parser.add_argument('--proton',type=pathlib.Path,required=True)
    args=parser.parse_args()
    args.output.mkdir(parents=True,exist_ok=True)
    for engine in ['wine','proton']:
        with tempfile.TemporaryDirectory(prefix='caskade-registry66-') as temporary:
            root=pathlib.Path(temporary)
            prefix=root/'prefix'
            (prefix/'drive_c').mkdir(parents=True)
            (prefix/'drive_c/caskade-fixture.reg').write_bytes(b'\xff\xfe'+import_text().encode('utf-16-le'))
            environment=dict(os.environ,WINEPREFIX=str(prefix),WINEDEBUG='-all',WINEDLLOVERRIDES='winemenubuilder.exe=d')
            if engine=='wine': command=[args.wine,'regedit.exe','/S',r'C:\caskade-fixture.reg']
            else:
                environment.update(PROTONPATH=str(args.proton),GAMEID='umu-default',UMU_RUNTIME_UPDATE='0',UMU_NO_PROTONFIXES='1',PRESSURE_VESSEL_SHARE_HOME='0',PRESSURE_VESSEL_FILESYSTEMS_RW=str(root),PRESSURE_VESSEL_FILESYSTEMS_RO=str(pathlib.Path.home()/'.local/share/umu')+':'+str(args.proton))
                command=[str(args.umu),'regedit.exe','/S',r'C:\caskade-fixture.reg']
            with (args.output/(engine+'.log')).open('w') as log:
                subprocess.run(command,env=environment,cwd=root,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=240)
                if engine=='wine':subprocess.run(['wineserver','-w'],env=environment,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=120)
            destination=args.output/engine
            destination.mkdir(exist_ok=True)
            for filename in ['system.reg','user.reg']:
                (destination/filename).write_text(extract(prefix/filename))
            print(engine+' authored sample exported',flush=True)
if __name__=='__main__':main()
