#!/usr/bin/env python3
"""Explicit trusted-runner cache provisioning; run as its unprivileged account."""
import argparse
import hashlib
import io
import os
from pathlib import Path
import subprocess
import shutil
import tarfile
import tempfile

VERSION='1.4.4'
URL=f'https://github.com/Open-Wine-Components/umu-launcher/releases/download/{VERSION}/umu-launcher-{VERSION}-zipapp.tar'
SHA256='eb590691841f7fad3fc3ad8fd5db4ccb87849fe7948e62b28ece7a4ee48cc851'
MAX_ARCHIVE=16*1024*1024

def unpack(data, destination):
    if len(data)>MAX_ARCHIVE or hashlib.sha256(data).hexdigest()!=SHA256:
        raise ValueError('umu archive differs from the product SHA-256 pin')
    total=0
    with tarfile.open(fileobj=io.BytesIO(data),mode='r:') as archive:
        for member in archive:
            path=Path(member.name)
            if path.is_absolute() or '..' in path.parts or not path.parts or path.parts[0]!='umu':
                raise ValueError('unsafe umu archive member')
            if member.isdir():
                (destination/path).mkdir(parents=True,exist_ok=True)
            elif member.isfile():
                total+=member.size
                if member.size>MAX_ARCHIVE or total>64*1024*1024:
                    raise ValueError('umu archive expansion exceeds the cache bootstrap limit')
                target=destination/path;target.parent.mkdir(parents=True,exist_ok=True)
                with archive.extractfile(member) as source, target.open('wb') as output:
                    while chunk:=source.read(65536):output.write(chunk)
                target.chmod((member.mode&0o755)|0o600)
            elif not (member.issym() or member.islnk()):
                raise ValueError('unexpected special umu archive member')
    if not (destination/'umu/umu-run').is_file():raise ValueError('umu-run missing')

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache',type=Path,required=True)
    args=parser.parse_args()
    if os.geteuid()==0:parser.error('umu/Steam Runtime must run as the dedicated unprivileged runner account')
    share=args.cache.resolve()/'native/share'
    launcher=share/f'caskade/umu/{VERSION}'
    if not (launcher/'umu/umu-run').is_file():
        from urllib.request import urlopen
        with urlopen(URL,timeout=60) as response:data=response.read(MAX_ARCHIVE+1)
        launcher.parent.mkdir(parents=True,exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='.umu-download-',dir=launcher.parent) as temporary:
            staged=Path(temporary)/'verified';staged.mkdir()
            unpack(data,staged)
            if launcher.exists():shutil.rmtree(launcher)
            os.replace(staged,launcher)
    environment=os.environ|{'XDG_DATA_HOME':str(share),'PROTONPATH':'UMU-Proton','GAMEID':'umu-default',
        'UMU_RUNTIME_UPDATE':'1','XDG_CACHE_HOME':str(args.cache.resolve()/'native/cache'),
        'XDG_CONFIG_HOME':str(args.cache.resolve()/'native/config'),'WINEDEBUG':'-all'}
    # Only the disposable warm-up prefix is writable game state. Cached runtime
    # downloads are the explicit purpose of this maintainer command.
    with tempfile.TemporaryDirectory(prefix='caskade-umu-cache-warm-') as temporary:
        environment['WINEPREFIX']=str(Path(temporary)/'prefix')
        subprocess.run([str(launcher/'umu/umu-run'),'wineboot','-u'],env=environment,check=True,timeout=1800)
        subprocess.run([str(launcher/'umu/umu-run'),'wineserver','-k'],env=environment,timeout=120,check=True)
    print(f'Warmed umu/UMU-Proton/Steam Runtime cache: {share}')
if __name__=='__main__':main()
